"""Freeze an authorized candidate, not live research sources. Offline, new-root only.

Usage: python3 freeze.py NEW_RELEASE CANDIDATE PRESENTATION_ROOT STORE_PACKAGE
The candidate must contain project/.research and already-captured project/sources.
The presentation root must contain graph.html and both presentation sidecars.
"""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.dont_write_bytecode = True
from inventory import encode, inventory, sha, tree_hash


def now():
    return datetime.now(timezone.utc).isoformat()


def run(args):
    return subprocess.run([str(x) for x in args], check=True, capture_output=True).stdout


def safe_files(path):
    if path.is_symlink():
        raise ValueError('no symlink source roots')
    files = []
    for p in [path, *sorted(path.rglob('*'))] if path.is_dir() else [path]:
        if p.is_symlink() or not (p.is_file() or p.is_dir()):
            raise ValueError('symlink/special source refused: ' + str(p))
        if p.is_file():
            files.append(p)
    return files


def main():
    if len(sys.argv) != 5:
        raise SystemExit(__doc__)
    root, candidate, presentation, package = [Path(p).absolute() for p in sys.argv[1:]]
    if root.exists() or root.is_symlink():
        raise ValueError('refusing existing release root')
    if root.name != 'alpha-1' or not str(package).startswith('/nix/store/'):
        raise ValueError('expected alpha-1 and exact installed store package')
    for ancestor in [root.parent, *root.parents]:
        if ancestor.is_symlink():
            raise ValueError('symlink destination ancestor')
    template = Path(__file__).resolve().parent
    launcher = (package / 'bin/rp-lookup').read_text().splitlines()[0]
    match = re.fullmatch(r'#!(/nix/store/[^ ]+/bin/python3) -I', launcher)
    if not match:
        raise ValueError('expected isolated paired store Python')
    python = match[1]
    as_of = '2026-09-12T12:00:00Z'
    # Snapshot metadata is an archived candidate product, not live source access.
    archived = json.loads((candidate / 'snapshot.json').read_text())
    artifacts = [o for o in archived['data']['objects'].values() if o['schema'] == 'rp/artifact-manifest/v1']
    copy = {}
    for p in safe_files(candidate / 'project/.research'):
        copy['project/' + p.relative_to(candidate / 'project').as_posix()] = p
    # Only file:sources paths actually declared by archived candidate artifacts.
    for artifact in artifacts:
        uri = artifact['uri']
        if not uri.startswith('file:sources/'):
            raise ValueError('artifact outside contained captured sources')
        rel = Path(uri[5:])
        if '..' in rel.parts or rel.is_absolute():
            raise ValueError('unsafe artifact source path')
        p = candidate / 'project' / rel
        if any(q.is_symlink() for q in [p, *p.parents]):
            raise ValueError('symlink artifact path')
        safe_files(p)
        if sha(p.read_bytes()) != artifact['sha256']:
            raise ValueError('archived artifact hash mismatch')
        copy['project/' + rel.as_posix()] = p
    for name in ['node-map.json', 'coverage.json', 'coverage.md', 'source-index.json', 'construction.json']:
        copy['references/' + name] = candidate / name
    for p in [candidate / 'project/name-map.json', candidate / 'name-map.json']:
        if p.exists() or p.is_symlink():
            copy['references/' + ('project-' if p.parent.name == 'project' else '') + p.name] = p
    for name in ['node-statuses.json', 'source-locators.json']:
        copy['references/' + name] = presentation / name
    for p in copy.values():
        safe_files(p)
    source_before = {str(p): sha(p.read_bytes()) for p in copy.values()}
    source_before[str(presentation / 'graph.html')] = sha((presentation / 'graph.html').read_bytes())
    created = now()
    os.umask(0o077)
    root.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if root.parent.stat().st_uid != os.getuid() or (root.parent.stat().st_mode & 0o777) != 0o700:
        raise ValueError('release parent must already be owned 0700 or newly created')
    root.mkdir(mode=0o700, exist_ok=False)
    for name in ['bin', 'references', 'evidence']:
        (root / name).mkdir(mode=0o700)
    def write(name, value):
        p = root / name
        p.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        p.write_bytes(value if isinstance(value, bytes) else encode(value))
        p.chmod(0o600)
    for name, source in copy.items():
        write(name, source.read_bytes())
    write('references/input-before.json', source_before)
    package_entries = inventory(package, private=False)
    write('references/package-inventory.json', package_entries)
    for name in ['rp', 'rp-view', 'rp-lookup']:
        (root / 'bin' / name).symlink_to(package / 'bin' / name)
    write('bin/rp-node', (template / 'rp-node.py').read_text().replace('@PYTHON@', python).encode())
    (root / 'bin/rp-node').chmod(0o700)  # sole regular executable, other files 0600
    lock = {'version': 'alpha-1', 'core_version': '0.1.0', 'as_of': as_of,
            'project': 'project', 'installed_package': str(package), 'python': python,
            'node_map': 'references/node-map.json', 'resolution': 'unique versioned map match -> exact revision ID'}
    write('references/release-lock.json', lock)
    for name in ['inventory.py']:
        write('references/' + name, (template / name).read_bytes())
    for name, command in [
        ('validation.json', [package / 'bin/rp', '--json', 'validate', '--project', root / 'project']),
        ('snapshot.json', [package / 'bin/rp', '--json', 'snapshot', '--project', root / 'project', '--as-of', as_of]),
        ('build-report.json', [package / 'bin/rp-view', '--project', root / 'project', '--as-of', as_of,
                              '--node-statuses', root / 'references/node-statuses.json',
                              '--source-locators', root / 'references/source-locators.json',
                              '--output', root / 'graph.html'])]:
        write('evidence/' + name, run(command))
    report = json.loads((root / 'evidence/build-report.json').read_text())
    assert report['canonical_count'] == 299 and report['html_bytes'] < 500000
    assert source_before == {p: sha(Path(p).read_bytes()) for p in source_before}
    assert all(sha((root / dest).read_bytes()) == source_before[str(src)] for dest, src in copy.items())
    write('evidence/input-preservation.json', {'checked_at': now(), 'original_inputs_unchanged': True,
          'copied_inputs_match': True, 'files_checked': len(source_before), 'before_inventory': 'references/input-before.json'})
    source_identity = {dest: {'source': str(src), 'sha256': source_before[str(src)]} for dest, src in copy.items()}
    write('references/snapshot-provenance.json', {'source_candidate': str(candidate), 'source_project': str(candidate / 'project'),
        'source_identity': source_identity, 'as_of': as_of, 'copied_at': created,
        'captured_at': json.loads((candidate / 'coverage.json').read_text())['captured_at'],
        'boundary': 'Already captured candidate only. No live original re-read; original source paths/digests are historical metadata.'})
    head = run(['git', 'rev-parse', 'HEAD']).decode().strip()
    dirty = bool(run(['git', 'status', '--porcelain=v1', '--untracked-files=normal']))
    nar = run(['nix-store', '-q', '--hash', package]).decode().strip()
    deriver = run(['nix-store', '-q', '--deriver', package]).decode().strip()
    write('manifest.json', {'version': 'alpha-1', 'core_version': '0.1.0', 'created_at': created,
        'host_observed': run(['hostname']).decode().strip(), 'entry': 'graph.html',
        'delivery_status': 'provisional-awaiting-parent-skills-and-final-verification',
        'release_identity': 'This manifest and its hash identify the private snapshot; unsigned, not scientific approval.',
        'installed_package': {'path': str(package), 'version_output': run([package / 'bin/rp', '--version']).decode().strip(),
            'deriver': deriver, 'nix_nar_hash': nar, 'tree_sha256': tree_hash(package_entries),
            'tree_inventory': 'references/package-inventory.json', 'gc_root_created': False,
            'portability': 'Store closure not copied. Requires this package and its Nix store dependencies to remain retained.'},
        'source_git_observation': {'head': head, 'dirty': dirty, 'observed_at': now(),
            'meaning': 'Repository context only; HEAD is not claimed to encode this dirty installed artifact or all changes.'},
        'snapshot': {'as_of': as_of, 'project_id': report['project_id'], 'canonical_records': 299,
            'nodes': 86, 'edges': 77, 'provenance': 'references/snapshot-provenance.json',
            'project_tree_sha256': tree_hash(inventory(root / 'project')),
            'node_map_sha256': sha((root / 'references/node-map.json').read_bytes()),
            'release_lock_sha256': sha((root / 'references/release-lock.json').read_bytes()),
            'graph_sha256': sha((root / 'graph.html').read_bytes()), 'graph_bytes': report['html_bytes']},
        'skills': {'status': 'absent-parent-owned', 'attachment': 'Parent adds private skills/ then runs references/inventory.py attach-skills; no source skills edited here.'},
        'permission_policy': 'Owned directories 0700, regular files 0600 except executable bin/rp-node 0700; store symlink target modes are unchanged.',
        'hash_convention': 'sha256 hex of bytes; tree hashes use inventory.py encode (sorted indented UTF-8 JSON with LF). Nix NAR hash separate.',
        'inventory_excludes': ['manifest.json', 'manifest.sha256'],
        'seal_note': 'manifest.sha256 covers manifest bytes; no recursive self-hash and no signature/authenticity claim.'})
    print(json.dumps({'created': str(root), 'html_bytes': report['html_bytes'], 'status': 'awaiting-docs-tests-and-initial-seal'}))


if __name__ == '__main__':
    main()
