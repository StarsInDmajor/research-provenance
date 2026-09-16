"""Read-only alpha acceptance; output JSON to stdout. No live source paths opened.
Usage: python3 -B verify-release.py RELEASE ORIGINAL_PRESENTATION_ROOT
"""
import html
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from inventory import inventory, sha


def decode(path):
    text = path.read_text()
    wire = json.loads(re.search(r'<script id="graph-data"[^>]*>(.*?)</script>', text, re.S)[1])
    records = json.loads(html.unescape(re.search(r'<pre id="canonical-records">(.*?)</pre>', text, re.S)[1]))
    return wire, records


def main():
    root, original = map(Path, sys.argv[1:3])
    before = inventory(root)
    lock = json.loads((root / 'references/release-lock.json').read_text())
    mapping = json.loads((root / 'references/node-map.json').read_text())
    command = root / 'bin/rp-node'
    query_count = 0
    # No caller environment or shell interpolation; hostile module and executables
    # in a scratch CWD with spaces must not be loaded by either isolated launcher.
    with tempfile.TemporaryDirectory(prefix='rp alpha hostile ') as scratch:
        scratch = Path(scratch)
        poison = "raise RuntimeError('caller module imported')\n"
        for module in ['json.py', 'subprocess.py', 'pathlib.py', 'sitecustomize.py']:
            (scratch / module).write_text(poison)
        for name in ['rp', 'rp-lookup', 'python3']:
            p = scratch / name
            p.write_text('#!/bin/sh\nexit 99\n'); p.chmod(0o700)
        env = {'PATH': str(scratch), 'PYTHONPATH': str(scratch), 'PYTHONHOME': str(scratch)}
        def call(query):
            nonlocal query_count
            query_count += 1
            p = subprocess.run([str(command), query], cwd=scratch, env=env, capture_output=True)
            return p.returncode, json.loads(p.stdout)
        for number in [79, 72]:
            logical, row = next((k, v) for k, v in mapping.items() if v['number'] == number)
            outputs = []
            for query in [f'R{number:02}', row['title'], logical, row['revision_id']]:
                code, data = call(query)
                assert code == 0 and data['status'] == 'selected' and data['selected_id'] == row['revision_id']
                assert data['as_of'] == lock['as_of']
                outputs.append(data)
            assert all(data == outputs[0] for data in outputs)
            raw = subprocess.run([str(root / 'bin/rp-lookup'), '--project', str(root / 'project'),
                '--as-of', lock['as_of'], '--', row['revision_id']], cwd=scratch, env=env, capture_output=True)
            assert raw.returncode == 0 and json.loads(raw.stdout) == outputs[0]
        max_relations = max_neighbors = 0
        for row in mapping.values():
            code, data = call(f"R{row['number']:02}")
            assert code == 0 and data['selected_id'] == row['revision_id']
            degree = len(data['incoming']) + len(data['outgoing'])
            max_relations = max(max_relations, degree)
            max_neighbors = max(max_neighbors, len(data['neighbors']))
            assert degree <= 60 and not data['truncated']
        for query in ['N01', 'R999', "'quoted'", '$(touch forbidden)', 'R79; touch forbidden']:
            code, data = call(query)
            assert code == 2 and data['status'] == 'not-found'
        assert not (scratch / 'forbidden').exists()
        for args in [['--project', '/tmp', 'R79'], ['--as-of', '2099-01-01T00:00:00Z', 'R79'], ['R79', 'R72']]:
            p = subprocess.run([str(command), *args], cwd=scratch, env=env, capture_output=True)
            assert p.returncode == 2
        assert subprocess.run([str(command), '--help'], cwd=scratch, env=env, capture_output=True).returncode == 0
    # Pure duplicate-map resolution test; never corrupt the actual frozen map.
    spec = importlib.util.spec_from_file_location('node_release', Path(__file__).with_name('rp-node.py'))
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    row = next(v for v in mapping.values() if v['number'] == 79)
    assert module.select_target({'a': row, 'b': row}, 'R79') == (None, 'ambiguous')
    oldwire, oldrecords = decode(original / 'graph.html')
    wire, records = decode(root / 'graph.html')
    assert wire == oldwire and records == oldrecords
    snapshot = json.loads((root / 'evidence/snapshot.json').read_text())
    assert snapshot['data']['objects'] == records
    assert len(records) == 299 and len(wire['graph']['nodes']) == 86 and len(wire['graph']['edges']) == 77
    # Only import the immutable installed locator validator, not checkout runtime.
    sys.path.insert(0, lock['installed_package'] + '/share/research-provenance/reader')
    import source_locators
    chunks = source_locators.unpack(wire['sourceLocators'])
    index = json.loads((root / 'references/source-index.json').read_text())
    sidecar = json.loads((root / 'references/source-locators.json').read_text())
    links = excerpt_nodes = 0
    locator_only = []
    for node in wire['graph']['nodes']:
        raw = records[node['id']]
        entry = sidecar['entries'][node['id']]
        # Installed helper defines the reader canonical format, separate from Core JCS.
        import build
        assert entry['canonical_digest'] == build.sha(build.compact(raw).encode())
        refs = node['sourceLocators']; sources = index[raw['logical_id']]['sources']
        assert len(refs) == len(sources)
        links += len(refs)
        excerpt_nodes += any(chunks[c]['excerpt'] for c, a in refs)
        if not any(chunks[c]['excerpt'] for c, a in refs):
            locator_only.append(raw['title'].split()[0])
        for source, (c, a) in zip(sources, refs):
            chunk = chunks[c]
            aid = wire['sourceLocators']['artifacts'][a]
            assert aid in raw['source']['artifacts']
            assert all(chunk[k] == source[k] for k in source_locators.FIELDS[:7])
            packet = (root / 'project' / records[aid]['uri'][5:]).read_bytes()
            assert sha(packet) == records[aid]['sha256']
            full = source_locators.captured_excerpt(packet, source)
            assert full.startswith(chunk['excerpt'])
            if chunk['excerpt']:
                assert sha(chunk['excerpt'].encode()) == chunk['display_sha256']
                assert len(chunk['excerpt'].encode()) <= 160
                assert chunk['display_end_line'] == source['start_line'] + len(chunk['excerpt'].splitlines()) - 1
    assert len(chunks) == 146 and links == 156 and excerpt_nodes == 85 and locator_only == ['R51']
    original_hashes = json.loads((root / 'references/input-before.json').read_text())
    assert all(sha(Path(p).read_bytes()) == digest for p, digest in original_hashes.items())
    assert inventory(root) == before
    print(json.dumps({'status': 'passed', 'queries': query_count, 'query_forms': ['Rxx', 'full title', 'logical ID', 'exact revision'],
        'hostile_cwd_path_pythonpath_pythonhome': 'passed', 'ambiguity': 'duplicate-map unit test passed; actual map unique',
        'unknown_and_quote_shell_text_rejection': 'passed', 'project_time_override_rejection': 'passed',
        'lookup_json_identical_to_paired_package': True, 'all_86_aliases_selected_exact': True,
        'maximum_one_hop_relations': max_relations, 'maximum_neighbors': max_neighbors,
        'canonical_records': 299, 'nodes': 86, 'edges': 77,
        'old_graph_wire_and_records_equal': True, 'release_snapshot_objects_equal_graph_records': True,
        'source_links': links, 'source_chunks': len(chunks), 'excerpt_nodes': excerpt_nodes, 'locator_only': locator_only,
        'original_inputs_unchanged': True, 'release_read_only_queries_and_verification': True,
        'html_bytes': (root / 'graph.html').stat().st_size, 'browser_gui_network_tests': 'not run'}))


if __name__ == '__main__':
    main()
