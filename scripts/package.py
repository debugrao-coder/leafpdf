"""Package a verified PDFium runtime, the release binary, and all notices."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cargo_metadata():
    cargo = shutil.which("cargo")
    if cargo is None:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        cargo = cargo_home / "bin" / ("cargo.exe" if os.name == "nt" else "cargo")
    result = subprocess.run(
        [str(cargo), "metadata", "--locked", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"],
        cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8",
    )
    return json.loads(result.stdout)


def copy_dependency_notices(metadata, destination):
    dependencies = []
    for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
        if package["source"] is None:
            continue
        package_root = Path(package["manifest_path"]).parent
        files = {path for path in package_root.iterdir()
                 if path.is_file() and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE"))}
        if package.get("license_file"):
            path = Path(package["license_file"])
            files.add(path if path.is_absolute() else package_root / path)
        if not files or not package.get("license"):
            raise ValueError(f"Missing dependency license information: {package['name']} {package['version']}")
        name = f"{package['name']}-{package['version']}"
        target = destination / name
        target.mkdir(parents=True)
        copied = {}
        for source in sorted(files):
            shutil.copyfile(source, target / source.name)
            copied[source.name] = digest(source)
        dependencies.append({"name": package["name"], "version": package["version"],
                             "license": package["license"], "repository": package.get("repository"),
                             "notices_sha256": copied})
    return dependencies


def copy_standard_library_notices(destination):
    rustc = shutil.which("rustc")
    if rustc is None:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        rustc = cargo_home / "bin" / ("rustc.exe" if os.name == "nt" else "rustc")
    sysroot = Path(subprocess.check_output([str(rustc), "--print", "sysroot"], text=True).strip())
    details = subprocess.check_output([str(rustc), "-vV"], text=True).strip().splitlines()
    fields = dict(line.split(": ", 1) for line in details[1:] if ": " in line)
    docs = sysroot / "share/doc/rust"
    copyright_file = docs / "COPYRIGHT-library.html"
    licenses = docs / "licenses"
    if not copyright_file.is_file() or not all((licenses / name).is_file() for name in ("MIT.txt", "Apache-2.0.txt")):
        raise ValueError("The rustc component must include standard-library copyright notices and license texts")
    destination.mkdir()
    shutil.copyfile(copyright_file, destination / copyright_file.name)
    shutil.copytree(licenses, destination / "licenses")
    return {"compiler": details[0], "commit": fields["commit-hash"], "host": fields["host"],
            "source": "https://github.com/rust-lang/rust/tree/" + fields["commit-hash"],
            "notices_sha256": {path.relative_to(destination).as_posix(): digest(path)
                               for path in sorted(destination.rglob("*")) if path.is_file()}}


def main():
    spec = importlib.util.spec_from_file_location("download_pdfium", Path(__file__).with_name("download-pdfium.py"))
    pdfium = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(pdfium)
    vendor = ROOT / "vendor"
    pdfium.verify_vendor(vendor)
    metadata = cargo_metadata()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    executable = Path(metadata["target_directory"]) / "release/leafpdf.exe"
    if not executable.is_file():
        raise FileNotFoundError("Build the Windows x64 release with cargo build --release --locked first")
    out = ROOT / "dist"
    out.mkdir(exist_ok=True)
    if out.is_symlink() or out.resolve().parent != ROOT:
        raise ValueError("Distribution directory must stay inside the repository")
    archive_name = "LeafPDF-Windows-x64.zip"
    with tempfile.TemporaryDirectory(prefix=".package-", dir=out) as temporary:
        stage = Path(temporary) / "LeafPDF"
        stage.mkdir()
        for source, name in ((executable, "leafpdf.exe"), (vendor / "bin/pdfium.dll", "pdfium.dll"),
                             (ROOT / "README.md", "README.md"), (ROOT / "LICENSE", "LICENSE"),
                             (ROOT / "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md")):
            shutil.copyfile(source, stage / name)
        notices = stage / "licenses"
        notices.mkdir()
        shutil.copyfile(vendor / "LICENSE", notices / "pdfium-binaries-LICENSE.txt")
        shutil.copytree(vendor / "licenses", notices / "pdfium")
        dependencies = copy_dependency_notices(metadata, notices / "rust")
        standard_library = copy_standard_library_notices(notices / "rust-std")
        (notices / "rust-dependencies.json").write_text(json.dumps(dependencies, indent=2) + "\n", encoding="utf-8")
        manifest = {"application": f"LeafPDF {version} Windows x64", "pdfium": pdfium.PIN,
                    "rust_standard_library": standard_library,
                    "pdfium_features": {"v8": False, "xfa": False},
                    "sha256": {path.relative_to(stage).as_posix(): digest(path)
                               for path in sorted(stage.rglob("*")) if path.is_file()}}
        (stage / "build-info.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        checksums = "".join(f"{digest(path)}  {path.relative_to(stage).as_posix()}\n"
                            for path in sorted(stage.rglob("*")) if path.is_file())
        (stage / "SHA256SUMS.txt").write_text(checksums, encoding="utf-8")
        archive = Path(temporary) / archive_name
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as output:
            for source in sorted(stage.rglob("*")):
                if source.is_file():
                    entry = zipfile.ZipInfo(str(source.relative_to(stage.parent)).replace("\\", "/"), (1980, 1, 1, 0, 0, 0))
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    entry.external_attr = 0o100644 << 16
                    output.writestr(entry, source.read_bytes(), compresslevel=9)
        archive_hash = digest(archive)
        destination = out / "LeafPDF"
        if destination.exists():
            if destination.is_symlink() or destination.resolve().parent != out.resolve():
                raise ValueError("Refusing to replace a distribution outside dist")
            shutil.rmtree(destination)
        stage.replace(destination)
        archive.replace(out / archive_name)
    (out / "SHA256SUMS.txt").write_text(f"{archive_hash}  {archive_name}\n", encoding="utf-8")
    print(json.dumps({"archive": f"dist/{archive_name}", "sha256": archive_hash,
                      "application": manifest["application"], "pdfium_version": pdfium.PIN["version"],
                      "dependency_count": len(dependencies), "checked_files": len(manifest["sha256"]) + 1,
                      "standard_library_notices": len(standard_library["notices_sha256"]),
                      "binary_sha256": {name: manifest["sha256"][name] for name in ("leafpdf.exe", "pdfium.dll")}}, indent=2))


if __name__ == "__main__":
    main()
