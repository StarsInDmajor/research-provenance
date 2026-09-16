#!@python@ -I
"""Installed generic entry points. Nix substitutes the isolated store interpreter."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def main():
    executable = Path(__file__).resolve()
    view = executable.name == 'rp-view'
    parser = argparse.ArgumentParser(
        prog=executable.name,
        allow_abbrev=False,
        description=(
            'Build a self-contained private HTML reader; never opens a browser or server.'
            if view else
            'Read-only exact title, revision ID or logical ID lookup; prints JSON, not aliases.'
        ),
        epilog=(
            'Trusted-local bounded projects only, not an audience-filtered public export. '
            'Artifact metadata only by default; canonical text and URI metadata remain private. '
            'Limits: 100 semantic revisions, 300 scientific relations, 512 canonical records, '
            '2,000,000 canonical bytes; stricter reader limits can reject smaller projects. '
            + ('HTML <500,000 bytes; source excerpts <=64 KiB/file, <=512,000 bytes total.'
               if view else
               'Query <=256 characters; <=8 candidates, <=60 one-hop relations, <=250 KB output. '
               'Exit 0 selected, 2 ambiguous/not-found/usage, 1 rejected.')
        ),
    )
    parser.add_argument('--project', required=True, help='RP project root (read-only)')
    parser.add_argument('--thread', help='Exact thread ID; required unless there is exactly one thread')
    parser.add_argument('--as-of', help='RFC3339 snapshot evaluation time (default: current UTC)')
    if view:
        parser.add_argument('--output', required=True, help='Private .html destination outside project; parent must be owned 0700 or newly created')
        parser.add_argument('--include-local-sources', action='store_true', help='Opt in to bounded, verified local file excerpts instead of artifact metadata only')
        parser.add_argument('--node-statuses', help='Explicit private presentation JSON (<=100 entries, <=64 KiB); not Core Assessment or freshness')
        parser.add_argument('--source-locators', help='Opt-in private archived per-node locator JSON (<=192000 bytes); reads only listed artifact-bound copied packets, never live originals')
        parser.add_argument('--force', action='store_true', help='Allow replacing unrelated 0600 regular output; containment/ownership protections still apply')
    else:
        parser.add_argument('name_or_id', help='Exact full title, exact revision ID, or logical ID (no universal short aliases)')
    args = parser.parse_args()

    # -I excludes cwd, user site, PYTHONPATH and PYTHONHOME before any imports.
    # Add only our installed allowlisted resources, never a checkout or caller path.
    package = executable.parent.parent
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(package / 'share/research-provenance/reader'))
    rp = package / 'bin/rp'
    try:
        if view:
            import build
            result = build.rebuild_generic(
                args.project, rp, args.output, thread=args.thread, as_of=args.as_of,
                force=args.force, include_local_sources=args.include_local_sources,
                node_statuses=args.node_statuses, source_locators=args.source_locators,
            )
        else:
            import lookup
            result = lookup.query_generic(
                args.project, rp, args.name_or_id, thread_id=args.thread, as_of=args.as_of,
            )
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print(json.dumps({'status': 'rejected', 'reason': 'Validation, identity, bounds or private output checks failed'}))
        return 1
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if view or result['status'] == 'selected' else 2


if __name__ == '__main__':
    raise SystemExit(main())
