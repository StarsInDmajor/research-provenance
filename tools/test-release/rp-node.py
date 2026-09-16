#!@PYTHON@ -I
"""Read-only convenience for one frozen private release; not a general alias registry."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys


def select_target(mapping, query):
    short = re.fullmatch(r'R([0-9]{2,})', query)
    matches = []
    for logical, entry in mapping.items():
        if ((short and entry['number'] == int(short[1]))
                or query in (logical, entry['title'], entry['revision_id'])):
            matches.append(entry['revision_id'])
    if len(matches) > 1:
        return None, 'ambiguous'
    if matches:
        return matches[0], None
    if re.fullmatch(r'[A-Z][0-9]+', query):
        return None, 'not-found'
    return query, None  # Exact package lookup, never fuzzy fallback or shell code.


def main():
    parser = argparse.ArgumentParser(allow_abbrev=False,
        description='alpha-1 read-only node JSON: Rxx, full title, logical ID or exact revision. Fixed snapshot; no project/time override.')
    parser.add_argument('node')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        lock = json.loads((root / 'references/release-lock.json').read_text())
        mapping = json.loads((root / 'references/node-map.json').read_text())
        target, error = select_target(mapping, args.node)
        if error:
            print(json.dumps({'status': error, 'reason': 'Release node-map must identify exactly one revision'}))
            return 2
        # Absolute paired launcher also uses -I and its own store resources.
        return subprocess.run([str(root / 'bin/rp-lookup'), '--project',
            str(root / 'project'), '--as-of', lock['as_of'], '--', target],
            check=False).returncode
    except (OSError, ValueError, KeyError, TypeError):
        print(json.dumps({'status': 'rejected', 'reason': 'Release lookup configuration unavailable or invalid'}))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
