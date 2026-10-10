"""Check that extraction rejects archive paths and links before writing files."""
import importlib.util
from pathlib import Path
import tarfile
import unittest

spec = importlib.util.spec_from_file_location(
    "download_pdfium", Path(__file__).with_name("download-pdfium.py")
)
download = importlib.util.module_from_spec(spec)
spec.loader.exec_module(download)


class ArchiveValidation(unittest.TestCase):
    def test_rejects_escape_and_windows_paths(self):
        for name in ("../outside", "/outside", "C:/outside", "bin\\outside", "bin/../outside", "bin/file:stream"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                download.validate_members([tarfile.TarInfo(name)])

    def test_rejects_links_and_case_collisions(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE):
            member = tarfile.TarInfo("bin/pdfium.dll")
            member.type = kind
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                download.validate_members([member])
        with self.assertRaises(ValueError):
            download.validate_members([tarfile.TarInfo("LICENSE"), tarfile.TarInfo("license")])

    def test_accepts_normal_files_and_directories(self):
        directory = tarfile.TarInfo("licenses")
        directory.type = tarfile.DIRTYPE
        files = [directory, tarfile.TarInfo("licenses/pdfium.txt"), tarfile.TarInfo("bin/pdfium.dll")]
        download.validate_members(files)


if __name__ == "__main__":
    unittest.main()
