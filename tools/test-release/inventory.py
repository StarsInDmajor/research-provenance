"""Private test-release hash inventory; no builds, GC roots, or deployment.

Create the initial manifest separately. `seal ROOT` adds an inventory only once.
`verify ROOT` is read-only. `attach-skills ROOT` permits new files only below
skills/, verifies the previous inventory first, and reseals for parent review.
Hashes identify bytes, not authorship, scientific acceptance, or signatures.
"""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys

EXCLUDED = {'manifest.json', 'manifest.sha256'}


def sha(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def encode(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()


def inventory(root, private=True):
    root = Path(root)
    result = {}
    if root.is_symlink() or not root.is_dir():
        raise ValueError('root must be a real directory')
    if private and (root.stat().st_uid != os.getuid() or stat.S_IMODE(root.stat().st_mode) != 0o700):
        raise ValueError('root must be owned 0700')
    for p in sorted(root.rglob('*')):
        name = p.relative_to(root).as_posix()
        if private and name in EXCLUDED:
            continue
        s = p.lstat()
        mode = stat.S_IMODE(s.st_mode)
        if private and s.st_uid != os.getuid():
            raise ValueError('foreign owner: ' + name)
        if p.is_symlink():
            target = os.readlink(p)
            # Release aliases reference individual paired store files only.
            if private and (not target.startswith('/nix/store/') or not p.resolve().is_file()):
                raise ValueError('unexpected release symlink: ' + name)
            result[name] = {'type': 'symlink', 'target': target,
                            'target_sha256': sha(p.read_bytes())}
        elif p.is_dir():
            if private and mode != 0o700:
                raise ValueError('directory not 0700: ' + name)
            result[name] = {'type': 'directory', 'mode': oct(mode)}
        elif p.is_file():
            allowed = 0o700 if name == 'bin/rp-node' else 0o600
            if private and mode != allowed:
                raise ValueError('unexpected file mode: ' + name)
            result[name] = {'type': 'file', 'mode': oct(mode), 'bytes': s.st_size,
                            'sha256': sha(p.read_bytes())}
        else:
            raise ValueError('special file: ' + name)
    return result


def tree_hash(entries):
    return sha(encode(entries))


def verify(root, manifest, allow_skills=False):
    expected = manifest['inventory']
    actual = inventory(root)
    for key, value in expected.items():
        if actual.get(key) != value:
            raise ValueError('changed/missing frozen entry: ' + key)
    added = set(actual) - set(expected)
    if added and (not allow_skills or any(p != 'skills' and not p.startswith('skills/') for p in added)):
        raise ValueError('unaccounted files outside authorized skills attachment')
    if tree_hash(expected) != manifest['inventory_tree_sha256']:
        raise ValueError('inventory tree hash mismatch')
    package = manifest['installed_package']
    package_entries = inventory(Path(package['path']), private=False)
    if tree_hash(package_entries) != package['tree_sha256']:
        raise ValueError('installed package tree changed')
    if package_entries != json.loads((root / 'references/package-inventory.json').read_text()):
        raise ValueError('package inventory mismatch')
    return actual, added


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in {'seal', 'verify', 'attach-skills'}:
        raise SystemExit('usage: inventory.py {seal|verify|attach-skills} RELEASE_ROOT')
    action = sys.argv[1]
    root = Path(sys.argv[2]).absolute()
    path = root / 'manifest.json'
    for p in [path, root / 'manifest.sha256']:
        if p.exists() and (p.is_symlink() or not p.is_file() or p.stat().st_uid != os.getuid()
                           or stat.S_IMODE(p.stat().st_mode) != 0o600):
            raise ValueError('manifest metadata must be owned regular 0600')
    raw = path.read_bytes()
    manifest = json.loads(raw)
    if action == 'seal':
        if 'inventory' in manifest or (root / 'manifest.sha256').exists():
            raise ValueError('refusing to replace existing seal')
        actual = inventory(root)
    else:
        if (root / 'manifest.sha256').read_text().strip() != sha(raw):
            raise ValueError('manifest hash mismatch')
        actual, added = verify(root, manifest, allow_skills=action == 'attach-skills')
        if action == 'verify':
            print(json.dumps({'status': 'verified', 'entries': len(actual),
                              'manifest_sha256': sha(raw), 'delivery_status': manifest['delivery_status']}))
            return
        if not added or not any(actual[p]['type'] == 'file' for p in added):
            raise ValueError('no new skills files to attach')
        manifest['skills'] = {'status': 'attached-pending-parent-final-verification',
                              'tree_sha256': tree_hash({k: v for k, v in actual.items() if k == 'skills' or k.startswith('skills/')})}
        manifest['delivery_status'] = 'skills-attached-pending-parent-final-verification'
    manifest['inventory'] = actual
    manifest['inventory_tree_sha256'] = tree_hash(actual)
    # Explicit seal actions only. verify and rp-node never write.
    path.write_bytes(encode(manifest))
    os.chmod(path, 0o600)
    (root / 'manifest.sha256').write_text(sha(path.read_bytes()) + '\n')
    os.chmod(root / 'manifest.sha256', 0o600)
    print(json.dumps({'status': 'sealed', 'entries': len(actual), 'delivery_status': manifest['delivery_status']}))


if __name__ == '__main__':
    main()
