"""Smoke-test the packaged executable using a synthetic PDF and isolated data."""
import json
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
output = root / "test-output/native"
result = output / "result.json"
output.mkdir(parents=True, exist_ok=True)
result.unlink(missing_ok=True)
environment = os.environ.copy()
environment["LEAFPDF_DATA_DIR"] = str(root / "test-output/isolated")
subprocess.run([str(root / "dist/LeafPDF/leafpdf.exe"),
                str(root / "test-output/多页阅读测试.pdf"), "--qa", str(output)],
               cwd=root, env=environment, check=True, timeout=90)
data = json.loads(result.read_text(encoding="utf-8"))
if data["pages"] != 7 or data["search_matches"] != list(range(7)):
    raise RuntimeError(f"Packaged reader QA failed: {data}")
if not all((output / f"view-{index}.png").is_file() for index in range(5)):
    raise RuntimeError("Packaged reader QA did not produce all five screenshots")
print(json.dumps(data, indent=2))
