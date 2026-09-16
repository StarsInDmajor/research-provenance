"""Explicit private-artifact acceptance, never imported by generic fixture suites.

Usage: python3 review-stage2-actual.py PRIVATE_ROOT BEFORE_HTML FINAL_HTML
Private science bytes remain at caller paths, never repository fixtures.
"""
import hashlib
import html
import json
from pathlib import Path
import re
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import build
import source_locators

ROOT, BEFORE, FINAL = map(Path,sys.argv[1:4]); sys.argv=sys.argv[:1]

def decoded(path):
    text=path.read_text()
    wire=json.loads(re.search(r'<script id="graph-data"[^>]*>(.*?)</script>',text,re.S)[1])
    records=json.loads(html.unescape(re.search(r'<pre id="canonical-records">(.*?)</pre>',text,re.S)[1]))
    return wire,records

class Actual(unittest.TestCase):
    def test_actual_full_graph_preserved_and_all_exact_sources_bound(self):
        self.assertTrue((ROOT/'source-locators.json').exists(), 'generated private sidecar missing')
        v=source_locators.strict_json((ROOT/'source-locators.json').read_bytes())
        old,oldrecords=decoded(BEFORE); new,records=decoded(FINAL)
        self.assertEqual(records,oldrecords); self.assertEqual(len(records),299)
        self.assertEqual(len(new['graph']['nodes']),86); self.assertEqual(len(new['graph']['edges']),77)
        loc=new.pop('sourceLocators'); chunks=source_locators.unpack(loc)
        self.assertEqual(len(chunks),146)
        index=json.loads((ROOT/'candidate-r2/source-index.json').read_text())
        links=0; mapped=0; excerpt_nodes=0
        for n in new['graph']['nodes']:
            refs=n.pop('sourceLocators'); mapped+=bool(refs); links+=len(refs)
            raw=records[n['id']]; entry=v['entries'][n['id']]
            self.assertEqual(entry['canonical_digest'],build.sha(build.compact(raw).encode()))
            src=index[raw['logical_id']]['sources']; self.assertEqual(len(src),len(refs))
            excerpt_nodes+=any(chunks[c]['excerpt'] for c,a in refs)
            for s,(c,a) in zip(src,refs):
                chunk=chunks[c]; aid=loc['artifacts'][a]
                self.assertIn(aid,raw['source']['artifacts'])
                for k in source_locators.FIELDS[:7]: self.assertEqual(chunk[k],s[k])
                packet=(ROOT/'candidate-r2/project'/records[aid]['uri'][5:]).read_bytes()
                self.assertEqual(build.sha(packet),records[aid]['sha256'])
                full=source_locators.captured_excerpt(packet,s)
                if chunk['excerpt']:
                    self.assertTrue(full.startswith(chunk['excerpt']))
                    self.assertEqual(build.sha(chunk['excerpt'].encode()),chunk['display_sha256'])
                    self.assertLessEqual(len(chunk['excerpt'].encode()),160)
                    self.assertEqual(chunk['display_end_line'],s['start_line']+len(chunk['excerpt'].splitlines())-1)
        self.assertEqual(mapped,86); self.assertEqual(links,156)
        self.assertEqual(new,old,'all preexisting wire fields including geometry/status must remain equal')
        self.assertLess(FINAL.stat().st_size,500000)
        print(json.dumps(dict(mapped=mapped,links=links,chunks=len(chunks),excerpt_nodes=excerpt_nodes,
            excerpts=sum(bool(c['excerpt']) for c in chunks),excerpt_bytes=sum(len(c['excerpt'].encode()) for c in chunks),
            html_bytes=FINAL.stat().st_size,headroom=500000-FINAL.stat().st_size)))

    def test_real_R02_and_R86_not_packet_wide_section_assignment(self):
        wire,records=decoded(FINAL)
        self.assertIn('sourceLocators',wire,'actual graph locator transport missing')
        chunks=source_locators.unpack(wire['sourceLocators'])
        def sources(logical):
            n=next(n for n in wire['graph']['nodes'] if records[n['id']]['logical_id']==logical)
            return [chunks[c] for c,a in n['sourceLocators']]
        r2=sources('reion3-parameter-order'); self.assertEqual(len(r2),1)
        self.assertEqual((r2[0]['path'],r2[0]['start_line'],r2[0]['end_line']),('src/simulation/config.py',10,42))
        r86=sources('reion3-historical-pt-stack'); self.assertEqual(len(r86),2)
        self.assertEqual((r86[0]['path'],r86[0]['start_line'],r86[0]['end_line']),
            ('docs/reports/2026-07-26-current-pt-posterior-structure-study.md',51,81))
        self.assertEqual(r86[1]['path'],'paper/README.md')
        self.assertNotEqual(r86[0]['section'],r86[1]['section'])
        self.assertTrue(all(s['source_type']=='metadata-only' and not s['sections'] for s in wire['sources'].values()))
        self.assertEqual(len(wire['sources']),24)
        self.assertEqual(sum('presentationStatus' in n for n in wire['graph']['nodes']),5)

if __name__=='__main__': unittest.main()
