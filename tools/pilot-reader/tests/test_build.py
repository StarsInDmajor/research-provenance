import base64
import hashlib
from html.parser import HTMLParser
import json
import re
from pathlib import Path
import os
import stat
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
from graph_projection import project
from test_projection import node, edge


class BuildSafetyTests(unittest.TestCase):
    def test_json_and_markup_escape(self):
        attack = '</script><img src=x onerror=alert(1)>\u2028\u2029'
        encoded = build.script_json({'title': attack})
        self.assertNotIn('<', encoded)
        self.assertNotIn('\u2028', encoded)
        self.assertEqual(json.loads(encoded)['title'], attack)
        self.assertNotIn('<img', build.text(attack))
        self.assertIn('&lt;img', build.text(attack))

    def test_inventory_and_observed_source_bytes(self):
        with tempfile.TemporaryDirectory(prefix='rp-node-reader-test-') as tmp:
            root = Path(tmp)
            (root/'source.md').write_bytes(b'bounded excerpt')
            observed = build.observe(root)
            self.assertEqual(build.verify_copy(observed, 'source.md', build.sha(b'bounded excerpt'), 15), 'bounded excerpt')
            (root/'source.md').write_bytes(b'changed')
            self.assertEqual(build.verify_copy(observed, 'source.md', build.sha(b'bounded excerpt'), 15), 'bounded excerpt')
            with self.assertRaisesRegex(ValueError, 'changed'):
                build.unchanged(root, observed)
            with self.assertRaises(ValueError):
                build.verify_copy(observed, '../source.md', build.sha(b'bounded excerpt'), 15)
            with self.assertRaises(ValueError):
                build.verify_copy(observed, 'source.md', build.sha(b'wrong'), 15)

    def test_symlinks_size_and_special_files_rejected(self):
        with tempfile.TemporaryDirectory(prefix='rp-node-reader-test-') as tmp:
            root = Path(tmp)
            (root/'link').symlink_to('/etc/passwd')
            with self.assertRaises(ValueError): build.observe(root)
            (root/'link').unlink()
            (root/'big').write_bytes(b'x'*(build.MAX_FILE+1))
            with self.assertRaises(ValueError): build.observe(root)
            (root/'big').unlink()
            os.mkfifo(root/'fifo')
            with self.assertRaises(ValueError): build.observe(root)

    def test_private_atomic_output_protects_input_and_shared_dirs(self):
        with tempfile.TemporaryDirectory(prefix='rp-node-reader-test-') as tmp:
            root = Path(tmp)
            source = root/'input'; source.mkdir()
            with self.assertRaises(ValueError):
                build.check_output(source/'reader-v3.html', [source])
            shared = root/'shared'; shared.mkdir(mode=0o755); shared.chmod(0o755)
            with self.assertRaises(ValueError):
                build.check_output(shared/'reader-v3.html', [source])
            self.assertEqual(stat.S_IMODE(shared.stat().st_mode), 0o755)
            output = root/'private'/'reader-v3.html'
            build.check_output(output, [source])
            build.atomic_write(output, 'old', lambda: None)
            self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(output.parent.stat().st_mode), 0o700)
            def fail(): raise ValueError('changed')
            with self.assertRaises(ValueError): build.atomic_write(output, 'new', fail)
            self.assertEqual(output.read_text(), 'old')
            output.unlink(); output.symlink_to(source/'target')
            with self.assertRaises(ValueError): build.check_output(output, [source])
            alias = root/'alias'; alias.symlink_to(source, target_is_directory=True)
            with self.assertRaises(ValueError): build.check_output(alias/'reader-v3.html', [source])

    def test_v5_explicit_allowlist_and_atomic_failure(self):
        with tempfile.TemporaryDirectory(prefix='rp-graph-output-') as tmp:
            root = Path(tmp)
            for name in ('reader-v3.html', 'reader-v4.html', 'reader-v5.html'):
                output = root/name
                build.check_output(output, [])
                build.atomic_write(output, 'preserved', lambda: None)
                def fail(): raise ValueError('final check rejected')
                with self.assertRaises(ValueError): build.atomic_write(output, 'bad', fail)
                self.assertEqual(output.read_text(), 'preserved')
            for name in ('reader.html', 'reader-v1.html', 'reader-v2.html', 'reader-v6.html', 'arbitrary.html'):
                with self.assertRaises(ValueError): build.check_output(root/name, [])
            (root/'reader-v4.html').unlink()
            (root/'reader-v4.html').symlink_to(root/'reader-v3.html')
            with self.assertRaises(ValueError): build.check_output(root/'reader-v4.html', [])
            self.assertEqual((root/'reader-v3.html').read_text(), 'preserved')

    def test_malformed_historical_csp_fails_independent_validator(self):
        # Literal historical defect, not constructed with the generator's helper.
        malformed = "'sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA='"
        self.assertIsNone(re.fullmatch(r"'sha256-[A-Za-z0-9+/]{43}='", malformed))

    def test_render_csp_svg_exact_keys_and_no_active_uris(self):
        a = node('a'); a['title'] = '</script><img src=x onerror=1>'
        a['statement'] = '原文 unchanged'
        a['untrusted_uri_text'] = 'javascript:alert(1)'
        a['untrusted_quote'] = '\" onload=\"alert(1)'
        a['untrusted_token'] = '@@JS@@'
        g = project([a, node('b'), edge('e', 'a', 'b')], 't')
        data = {'graph': g, 'sources': {}, 'notes': {}, 'records': {r['id']:r for r in [a,node('b'),edge('e','a','b')]}}
        html = build.render(data, {'as_of':'2026-09-08T23:59:59Z','generated_at':'2026-09-09T00:00:00Z'})
        self.assertIn('<svg', html)
        from wire_fixture import decode_html
        decoded = decode_html(html)
        self.assertEqual(decoded['graph'], g)
        self.assertEqual(decoded['records'], data['records'])
        # Runtime SVG exact keys/endpoints are checked against static oracle in JS.
        self.assertNotIn('data-key=', html)
        self.assertNotIn('<img', html)
        self.assertIn('原文 unchanged', html)
        self.assertIn('<noscript>', html)
        class Parse(HTMLParser):
            def __init__(self): super().__init__(); self.scripts=[]; self.inside=False; self.csp=''
            def handle_starttag(self, tag, attrs):
                attrs=dict(attrs)
                for key in attrs:
                    self_outer.assertFalse(key.startswith('on'))
                if tag=='script': self.scripts.append(''); self.inside=True
                if tag=='meta' and attrs.get('http-equiv')=='Content-Security-Policy': self.csp=attrs['content']
                if 'href' in attrs: self_outer.assertTrue(attrs['href'].startswith('#'))
                self_outer.assertNotIn('src', attrs)
            def handle_endtag(self, tag):
                if tag=='script': self.inside=False
            def handle_data(self, content):
                if self.inside: self.scripts[-1]+=content
        self_outer=self
        parser=Parse(); parser.feed(html)
        self.assertEqual(len(parser.scripts),3)
        directives = {parts[0]: parts[1:] for clause in parser.csp.split(';')
                      if (parts := clause.split())}
        sources = directives['script-src']
        # CSP hash-source grammar uses a hyphen, not the artifact digest colon.
        # Check grammar independently before testing the exact embedded bytes.
        self.assertEqual(len(sources), 3)
        for source in sources:
            self.assertRegex(source, r"^'sha256-[A-Za-z0-9+/]{43}='$")
            self.assertEqual(len(base64.b64decode(source[8:-1], validate=True)), 32)
        for script in parser.scripts:
            digest=base64.b64encode(hashlib.sha256(script.encode()).digest()).decode()
            self.assertIn("'sha256-"+digest+"'", sources)
        self.assertNotIn("'unsafe-inline'", sources)
        self.assertNotIn('unsafe-eval', parser.csp)
        self.assertLess(len(html.encode()),500000)
        self.assertEqual(html,build.render(data, {'as_of':'2026-09-08T23:59:59Z','generated_at':'2026-09-09T00:00:00Z'}))


if __name__=='__main__': unittest.main()
