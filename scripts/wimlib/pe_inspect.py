"""PE metadata through the workspace's pinned HexSpell parser."""
import json
from pathlib import Path
import subprocess


def inspect_pe(path):
    root = Path(__file__).resolve().parents[3] / 'windows-uup'
    result = subprocess.run(
        ['cargo', 'run', '--quiet', '--locked', '-p', 'windows-delta',
         '--example', 'pe_inspect', '--', str(Path(path).resolve())],
        cwd=root, check=True, capture_output=True, text=True,
    )
    return json.loads(result.stdout)
