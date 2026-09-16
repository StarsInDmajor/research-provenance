"""Serialize generated HTML structure for the strict Node DOM double, no browser."""
from html.parser import HTMLParser
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
from graph_projection import project
from test_projection import node, edge, binding


class Tree(HTMLParser):
    def __init__(self):
        super().__init__()
        self.root = dict(tag='#document', attrs={}, text='', children=[])
        self.stack = [self.root]
        self.scripts = []

    def handle_starttag(self, tag, attrs):
        # HTMLParser lowercases attributes; HTML's SVG foreign-content parser
        # restores these names. The strict double must see actual DOM casing.
        svg_names = {'viewbox': 'viewBox', 'preserveaspectratio': 'preserveAspectRatio'}
        attrs = [(svg_names.get(k, k), v) for k, v in attrs]
        item = dict(tag=tag, attrs=dict(attrs), text='', children=[])
        self.stack[-1]['children'].append(item)
        if tag == 'script':
            self.scripts.append(item)
        if tag not in {'meta', 'input', 'br', 'hr', 'img', 'link'}:
            self.stack.append(item)

    def handle_endtag(self, tag):
        if self.stack[-1]['tag'] == tag:
            self.stack.pop()

    def handle_data(self, text):
        self.stack[-1]['text'] += text


def generated():
    a, b, old = node('a'), node('b'), node('old', logical='a')
    a['logical_id'] = 'a'
    a['revision']['parents'] = [{'id': 'old'}]
    a['statement'] = 'new statement'
    old['statement'] = 'old statement'
    e = edge('e', 'a', 'b')
    e['type'] = 'depends-on'
    e['rationale'] = 'exact reason'
    e['scope'] = {'statement': 'exact scope'}
    records = [a, b, old, e, binding('ba','a','primary'), binding('bb','b','alternative')]
    data = dict(graph=project(records, 't'), sources={}, notes={}, records={r['id']: r for r in records})
    return build.render(data, dict(as_of='2026-09-08T23:59:59Z', generated_at='2026-09-10T00:00:00Z'))


if __name__ == '__main__':
    html = Path(sys.argv[1]).read_text() if len(sys.argv) > 1 else generated()
    parser = Tree()
    parser.feed(html)
    print(json.dumps(dict(tree=parser.root, scripts=[s['text'] for s in parser.scripts if s['attrs'].get('type') != 'application/json'])))
