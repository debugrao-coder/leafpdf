"""Fetch the pinned Windows x64 PDFium archive and preserve its notices."""
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PIN = json.loads(Path(__file__).with_name("pdfium.json").read_text(encoding="utf-8"))
ARCHIVE_NAME = "pdfium-win-x64.tgz"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def validate_members(members):
    seen = set()
    total = 0
    for member in members:
        path = PurePosixPath(member.name)
        if (not member.name or path.is_absolute() or "\\" in member.name
                or ":" in member.name or ".." in path.parts
                or any(part.endswith((" ", ".")) for part in path.parts)
                or not (member.isfile() or member.isdir())):
            raise ValueError(f"Unsafe PDFium archive member: {member.name!r}")
        normalized = str(path).casefold()
        if normalized in seen:
            raise ValueError(f"Duplicate PDFium archive member: {member.name!r}")
        seen.add(normalized)
        total += member.size
        if total > 128 * 1024 * 1024:
            raise ValueError("PDFium archive exceeds the extraction size limit")


def verify_vendor(vendor):
    if vendor.is_symlink() or not vendor.resolve().is_relative_to(ROOT):
        raise ValueError("PDFium vendor directory must stay inside the repository")
    blob = (vendor / ARCHIVE_NAME).read_bytes()
    if sha256(blob) != PIN["archive_sha256"]:
        raise ValueError("PDFium archive SHA256 does not match scripts/pdfium.json")
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as archive:
        members = archive.getmembers()
        validate_members(members)
        expected = {member.name for member in members if member.isfile()} | {ARCHIVE_NAME}
        actual_files = {path.relative_to(vendor).as_posix() for path in vendor.rglob("*") if path.is_file()}
        if actual_files != expected or any(path.is_symlink() for path in vendor.rglob("*")):
            raise ValueError("PDFium vendor directory contains missing, extra, or linked files")
        for member in members:
            if member.isfile():
                source = archive.extractfile(member).read()
                actual = vendor.joinpath(*PurePosixPath(member.name).parts)
                if (not actual.is_file() or actual.is_symlink()
                        or not actual.resolve().is_relative_to(vendor.resolve())
                        or actual.read_bytes() != source):
                    raise ValueError(f"PDFium file differs from the verified archive: {member.name}")
    version = dict(line.split("=", 1) for line in (vendor / "VERSION").read_text().splitlines() if "=" in line)
    actual_version = ".".join(version[key] for key in ("MAJOR", "MINOR", "BUILD", "PATCH"))
    if actual_version != PIN["version"]:
        raise ValueError("Unexpected PDFium version")
    if sha256((vendor / "bin/pdfium.dll").read_bytes()) != PIN["dll_sha256"]:
        raise ValueError("Unexpected PDFium DLL SHA256")
    args = (vendor / "args.gn").read_text().replace(" ", "")
    if "pdf_enable_v8=false" not in args or "pdf_enable_xfa=false" not in args:
        raise ValueError("Expected the no-V8/no-XFA PDFium build")


def main():
    request = urllib.request.Request(PIN["url"], headers={"User-Agent": "LeafPDF-build/0.1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        blob = response.read(16 * 1024 * 1024 + 1)
    if len(blob) > 16 * 1024 * 1024 or sha256(blob) != PIN["archive_sha256"]:
        raise ValueError("Downloaded PDFium archive SHA256 does not match the pinned value")
    # Validate every member before writing anything; do not follow tar links.
    with tempfile.TemporaryDirectory(prefix=".pdfium-", dir=ROOT) as temporary:
        stage = Path(temporary) / "vendor"
        stage.mkdir()
        with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as archive:
            members = archive.getmembers()
            validate_members(members)
            for member in members:
                target = stage.joinpath(*PurePosixPath(member.name).parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with archive.extractfile(member) as source, target.open("wb") as output:
                        shutil.copyfileobj(source, output)
        (stage / ARCHIVE_NAME).write_bytes(blob)
        verify_vendor(stage)
        vendor = ROOT / "vendor"
        if vendor.exists():
            if vendor.is_symlink() or vendor.resolve().parent != ROOT:
                raise ValueError("Refusing to replace a vendor directory outside the repository")
            shutil.rmtree(vendor)
        stage.replace(vendor)
    print(json.dumps({"version": PIN["version"], "url": PIN["url"], "sha256": PIN["archive_sha256"]}, indent=2))


if __name__ == "__main__":
    main()
