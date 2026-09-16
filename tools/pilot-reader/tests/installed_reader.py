"""Standalone stdlib-only installed-package contract; no checkout imports or fixtures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

AS_OF = '2026-09-12T00:00:00Z'
RESOURCES = {
    'build.py', 'lookup.py', 'case_io.py', 'source_locators.py', 'graph_projection.py', 'routing.py',
    'routing.js', 'template.html', 'reader.css', 'graph.js', 'bootstrap.js', 'svg.js',
}


def fixture(root, suffix):
    """Two independent domain-neutral block-YAML projects, not research claims."""
    rid = 'qst_01J000000000000000000000' + suffix
    tid = 'thd_01J000000000000000000000' + suffix
    title = 'Synthetic packaging question ' + suffix
    body = f'SYNTHETIC_SOURCE_BODY_{suffix}\n'
    source = root / 'data' / 'synthetic.txt'
    source.parent.mkdir(parents=True)
    source.write_text(body)
    files = {
        f'artifacts/synthetic--art_01J000000000000000000000{suffix}.yaml': f'''schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000{suffix}
title: Synthetic source body {suffix}
uri: file:data/synthetic.txt
media_type: text/plain
size_bytes: {len(body.encode())}
sha256: sha256:{hashlib.sha256(body.encode()).hexdigest()}
created_at: '{AS_OF}'
access:
  level: internal
  compartments: []
''',
        'project.yaml': f'''schema: rp/project/v1
id: proj_01J000000000000000000000{suffix}
slug: synthetic-{suffix.lower()}
title: Synthetic packaging fixture {suffix}
created_at: '{AS_OF}'
access_defaults:
  level: internal
  compartments: []
repository_policy:
  visibility: private
  allowed_remotes: []
schema_policy:
  core_version: v1
  kind_registry: rp/kinds/v1
  allowed_extensions: []
''',
        f'threads/synthetic--{tid}.yaml': f'''schema: rp/research-thread/v1
id: {tid}
title: Synthetic thread {suffix}
objective: Exercise installed packaging only
created_at: '{AS_OF}'
created_by:
  type: human
  id: synthetic-tester
root_question_revision: {rid}
parent_thread_id: null
forked_from_thread_id: null
fork_reason: null
access:
  level: internal
  compartments: []
''',
        f'records/questions/synthetic--{rid}.yaml': f'''schema: rp/node-revision/v1
id: {rid}
logical_id: synthetic-question-{suffix.lower()}
kind: Question
record_state: frozen
title: {title}
statement: Does the installed launcher preserve this synthetic record?
scope:
  statement: Packaging tests only; no scientific claim
question:
  resolution_criteria:
  - Installed lookup returns the exact synthetic revision
  evidence_requirements:
  - Installed executable output
assumptions: []
limitations:
- Synthetic packaging test, not real research
revision:
  parents: []
  summary: Initial synthetic fixture
created_at: '{AS_OF}'
created_by:
  type: human
  id: synthetic-tester
source:
  revisions: []
access:
  level: internal
  compartments: []
tags: []
''',
    }
    for name, text in files.items():
        target = root / '.research' / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding='utf-8')
    return rid, tid, title


def inventory(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in root.rglob('*') if p.is_file()}


class InstalledReader(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='rp-installed-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.cwd = self.root / 'hostile cwd 空间'
        self.cwd.mkdir()
        marker = self.root / 'EXECUTED'
        poison = f"from pathlib import Path\nPath({str(marker)!r}).touch()\nraise RuntimeError('shadow import executed')\n"
        for name in ('json', 'subprocess', 'pathlib', 'argparse', 'build', 'lookup',
                     'case_io', 'sitecustomize', 'usercustomize', 'webbrowser'):
            (self.cwd / (name + '.py')).write_text(poison)
        for name in ('rp', 'xdg-open', 'firefox', 'chromium'):
            path = self.cwd / name
            path.write_text(f'#!{sys.executable} -I\n' + poison)
            path.chmod(0o755)
        self.env = os.environ.copy()
        self.env.update(PYTHONPATH=str(self.cwd), PYTHONHOME=str(self.cwd),
                        PATH=str(self.cwd), BROWSER=str(self.cwd / 'xdg-open'))
        self.addCleanup(lambda: self.assertFalse(marker.exists(), 'hostile code/browser executed'))

    def run_cli(self, name, *args, code=0, package=None):
        exe = (package or PACKAGE) / 'bin' / name
        self.assertTrue(exe.is_file(), f'missing installed executable: {name}')
        result = subprocess.run([str(exe), *map(str, args)], cwd=self.cwd,
                                env=self.env, capture_output=True, text=True, timeout=90)
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        return result

    def test_installed_allowlist_and_help(self):
        self.assertEqual({p.name for p in (PACKAGE / 'bin').iterdir()},
                         {'rp', 'rp-view', 'rp-lookup', 'rp-server'})
        self.assertEqual({p.name for p in (PACKAGE / 'share/research-provenance/reader').iterdir()}, RESOURCES)
        for name in ('rp-view', 'rp-lookup'):
            help_text = self.run_cli(name, '--help').stdout
            self.assertIn('--project', help_text)
            self.assertIn('metadata', help_text)
            for forbidden in ('--rp', '--before', '--case-manifest'):
                self.assertNotIn(forbidden, help_text)
                required = ('--output', self.root / 'graph.html') if name == 'rp-view' else ('some title',)
                self.run_cli(name, '--project', self.root, *required, forbidden, 'unexpected', code=2)
        self.run_cli('rp-view', '--proj', self.root, '--output', self.root / 'x.html', code=2)
        self.run_cli('rp', '--version')

    def test_render_lookup_and_relocation(self):
        for suffix in ('A1', 'B2'):
            with self.subTest(fixture=suffix):
                project = self.root / ('project 空间 ' + suffix)
                rid, tid, title = fixture(project, suffix)
                before = inventory(project)
                package = PACKAGE
                if suffix == 'B2':
                    package = self.root / 'relocated package 空间'
                    shutil.copytree(PACKAGE, package)
                self.run_cli('rp', 'validate', '--project', project, '--json', package=package)
                output = self.root / ('private ' + suffix) / 'graph 空间 ;.html'
                args = ('--project', project, '--as-of', AS_OF, '--thread', tid)
                report = json.loads(self.run_cli('rp-view', *args, '--output', output, package=package).stdout)
                self.assertEqual(report['mode'], 'generic-snapshot')
                self.assertEqual(report['sources']['rp_bin'], 'sha256:' + hashlib.sha256((package / 'bin/rp').read_bytes()).hexdigest())
                html = output.read_text()
                self.assertIn(title, html)
                self.assertNotIn('SYNTHETIC_SOURCE_BODY_' + suffix, html)
                self.assertIn('id="workspace"', html)
                self.assertIn('id="canonical-records"', html)
                self.assertIn('"wireVersion":1', html)
                self.assertNotIn('data-key=', html)
                self.assertLess(len(html.encode()), 500000)
                self.assertIn('Content-Security-Policy', html)
                self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
                self.assertEqual(stat.S_IMODE(output.parent.stat().st_mode), 0o700)
                # Existing reader replacement and optional source mode retain generic dispatch.
                self.run_cli('rp-view', *args, '--output', output, '--include-local-sources', package=package)
                self.assertIn('SYNTHETIC_SOURCE_BODY_' + suffix, output.read_text())
                # Profile-style symlinks still resolve the paired package resources.
                profile = self.root / ('profile-' + suffix)
                (profile / 'bin').mkdir(parents=True)
                for name in ('rp-view', 'rp-lookup'):
                    (profile / 'bin' / name).symlink_to(package / 'bin' / name)
                self.run_cli('rp-view', *args, '--output', output, package=profile)
                self.run_cli('rp-lookup', *args, title, package=profile)
                for query in (title, rid, 'synthetic-question-' + suffix.lower()):
                    found = json.loads(self.run_cli('rp-lookup', *args, query, package=package).stdout)
                    self.assertEqual(found['selected_id'], rid)
                    self.assertIsNone(found['candidates'][0]['alias'])
                    self.assertNotIn('SYNTHETIC_SOURCE_BODY_' + suffix, json.dumps(found))
                missing = json.loads(self.run_cli('rp-lookup', *args, 'not a title', code=2, package=package).stdout)
                self.assertEqual(missing['status'], 'not-found')
                self.assertEqual(inventory(project), before)

    def test_explicit_node_statuses_paired_launcher(self):
        self.assertIn('--node-statuses', self.run_cli('rp-view', '--help').stdout)
        project = self.root / 'history-project'
        rid, _, _ = fixture(project, 'A1')
        snap = json.loads(self.run_cli('rp', 'snapshot', '--project', project, '--json').stdout)['data']
        digest = lambda r: 'sha256:' + hashlib.sha256(json.dumps(r, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        source = next(r for r in snap['objects'].values() if r['schema'] == 'rp/artifact-manifest/v1')
        status = dict(revision_id=rid, canonical_digest=digest(snap['objects'][rid]), status='historical',
                      label='历史记录', reason='Synthetic earlier record; no scientific claim', assessed_at=AS_OF,
                      source_refs=[dict(id=source['id'], canonical_digest=digest(source), locator='synthetic packet lines 1–2')])
        value = dict(version=1, project_id=snap['project']['id'], entries={rid:status})
        path = self.root / 'statuses.json'; path.write_text(json.dumps(value))
        output = self.root / 'history-view' / 'graph.html'
        before = inventory(project)
        args = ('--project', project, '--output', output, '--node-statuses', path, '--as-of', AS_OF)
        report = json.loads(self.run_cli('rp-view', *args).stdout)
        self.assertEqual(report['node_statuses']['entries'], 1)
        self.assertIn('Synthetic earlier record', output.read_text())
        saved = output.read_bytes()
        status['canonical_digest'] = 'sha256:' + '0'*64; path.write_text(json.dumps(value))
        self.run_cli('rp-view', *args, code=1)
        self.assertEqual(saved, output.read_bytes())
        self.assertEqual(before, inventory(project))

    def test_explicit_source_locators_paired_launcher(self):
        self.assertIn('--source-locators',self.run_cli('rp-view','--help').stdout)
        project=self.root/'source-project'; rid,_,_=fixture(project,'A1')
        snap=json.loads(self.run_cli('rp','snapshot','--project',project,'--json').stdout)['data']
        digest=lambda r:'sha256:'+hashlib.sha256(json.dumps(r,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        sha=lambda b:'sha256:'+hashlib.sha256(b).hexdigest()
        node=snap['objects'][rid]; art=next(r for r in snap['objects'].values() if r['schema']=='rp/artifact-manifest/v1')
        body='中文 </pre></script>\nsecond archived line\n'
        source=dict(path='src/synthetic.py',start_line=10,end_line=11,section='# Synthetic',sha256=sha(b'original capture'),excerpt_sha256=sha(body.encode()),captured_at=AS_OF)
        packet=(f"## src/synthetic.py:L10-L11\nOriginal file {source['sha256']}; excerpt {source['excerpt_sha256']}; captured {AS_OF}\n\n```text\n"+body+'\n```\n').encode()
        target=project/'data/synthetic.txt';target.write_bytes(packet)
        art.update(sha256=sha(packet),size_bytes=len(packet));node['source']['artifacts']=[art['id']]
        next((project/'.research/artifacts').glob('*.yaml')).write_text(json.dumps(art))
        next((project/'.research/records/questions').glob('*.yaml')).write_text(json.dumps(node))
        snap=json.loads(self.run_cli('rp','snapshot','--project',project,'--json').stdout)['data']
        node=snap['objects'][rid];art=snap['objects'][art['id']]
        value=dict(version=1,project_id=snap['project']['id'],entries={rid:dict(revision_id=rid,
            canonical_digest=digest(node),sources=[dict(artifact_id=art['id'],artifact_digest=digest(art),packet_path='data/synthetic.txt',**source)])})
        path=self.root/'locators.json';path.write_text(json.dumps(value))
        output=self.root/'source-view/graph.html';before=inventory(project)
        args=('--project',project,'--output',output,'--as-of',AS_OF,'--source-locators',path)
        report=json.loads(self.run_cli('rp-view',*args).stdout)
        self.assertEqual(report['source_locators']['mapped_nodes'],1)
        self.assertEqual(report['source_locators']['excerpts'],1)
        self.assertIn('src/synthetic.py',output.read_text());self.assertNotIn(body,output.read_text())
        self.assertIn(r'\u003c/pre\u003e',output.read_text())
        old=output.read_bytes()
        for bad in ({**value,'project_id':'wrong'},dict(value,entries=[])):
            path.write_text(json.dumps(bad));self.run_cli('rp-view',*args,code=1);self.assertEqual(old,output.read_bytes())
        path.unlink();self.run_cli('rp-view',*args,code=1);self.assertEqual(old,output.read_bytes())
        self.assertEqual(before,inventory(project))

    def test_compact_large_records_and_cap_rejection_preserve_old_output(self):
        """Neutral installed regression for redundant raw encoding, not science."""
        project = self.root / 'large synthetic'
        rid, tid, _ = fixture(project, 'A1')
        snap = json.loads(self.run_cli('rp', 'snapshot', '--project', project, '--json').stdout)['data']
        seed = snap['objects'][rid]
        directory = project / '.research/records/questions'
        for path in directory.iterdir():
            path.unlink()
        for i in range(78):
            record = dict(seed, id=rid if i == 0 else f'qst_{i:026d}',
                          logical_id=f'synthetic-large-{i}', title=f'Synthetic packaging record {i}',
                          statement=f'COMPLETE_RAW_{i:02d} ' + 'Synthetic payload only. ' * 90)
            (directory / f'synthetic-{i}--{record["id"]}.yaml').write_text(json.dumps(record))
        output = self.root / 'private-large' / 'graph.html'
        args = ('--project', project, '--output', output, '--as-of', AS_OF)
        before = inventory(project)
        self.run_cli('rp-view', *args)
        html = output.read_text()
        from html.parser import HTMLParser
        class Raw(HTMLParser):
            active = False
            text = ''
            def handle_starttag(self, tag, attrs):
                if dict(attrs).get('id') == 'canonical-records': self.active = True
            def handle_endtag(self, tag):
                if tag == 'pre': self.active = False
            def handle_data(self, value):
                if self.active: self.text += value
        parsed = Raw(); parsed.feed(html)
        expected = json.loads(self.run_cli('rp', 'snapshot', '--project', project, '--json').stdout)['data']['objects']
        self.assertEqual(json.loads(parsed.text), expected)
        self.assertLess(len(html.encode()), 500000)
        self.assertNotIn('data-key=', html)
        self.assertEqual(inventory(project), before)
        saved = output.read_bytes()
        # Still below canonical 2MB/512-record limits but genuinely above HTML
        # cap. Neither --force nor compaction may weaken atomic publication.
        for path in directory.iterdir():
            record = json.loads(path.read_text()); record['statement'] += 'x' * 6000
            path.write_text(json.dumps(record))
        before = inventory(project)
        self.run_cli('rp-view', *args, '--force', code=1)
        self.assertEqual(output.read_bytes(), saved)
        self.assertEqual(inventory(project), before)

    def test_rejection_preserves_files(self):
        project = self.root / 'project'
        rid, _, _ = fixture(project, 'A1')
        output = self.root / 'private' / 'graph.html'
        output.parent.mkdir(mode=0o700)
        output.write_text('unrelated private file')
        output.chmod(0o600)
        args = ('--project', project, '--output', output, '--as-of', AS_OF)
        self.run_cli('rp-view', *args, code=1)
        self.assertEqual(output.read_text(), 'unrelated private file')
        self.run_cli('rp-view', *args, '--force')
        saved = output.read_bytes()
        self.run_cli('rp-lookup', '--project', project, 'x' * 257, code=1)
        self.run_cli('rp-view', *args, '--as-of', 'invalid-time', code=1)
        self.run_cli('rp-view', '--project', project, '--output', project / 'inside.html', '--force', code=1)
        self.assertFalse((project / 'inside.html').exists())
        link = output.parent / 'link.html'
        link.symlink_to(output)
        self.run_cli('rp-view', '--project', project, '--output', link, '--force', code=1)
        self.assertTrue(link.is_symlink())
        self.assertEqual(output.read_bytes(), saved)
        canonical = next((project / '.research/records/questions').glob('*.yaml'))
        canonical.write_text('schema: [malformed')
        before = inventory(project)
        self.run_cli('rp-view', *args, '--force', code=1)
        self.run_cli('rp-lookup', '--project', project, rid, code=1)
        self.assertEqual(output.read_bytes(), saved)
        self.assertEqual(inventory(project), before)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    args, rest = parser.parse_known_args()
    PACKAGE = args.package.resolve()
    unittest.main(argv=[sys.argv[0], *rest])
