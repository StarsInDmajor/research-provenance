"""Shared final-artifact reader for Python assertions, old + compact formats.

Actual production JS decode/boot is separately exercised by the strict DOM suites.
This independent adapter restores the same projection; never changes raw records.
"""
import json
from entry_fixture import Tree


def elements(tree):
    yield tree
    for child in tree['children']:
        yield from elements(child)


def decode_html(html):
    parser = Tree(); parser.feed(html)
    by = {e['attrs']['id']:e for e in elements(parser.root) if 'id' in e['attrs']}
    data = json.loads(by['graph-data']['text'])
    if 'wireVersion' not in data:
        return data
    assert data['wireVersion'] == 1
    assert 'records' not in data
    data['records'] = json.loads(by['canonical-records']['text'])
    for n in data['graph']['nodes']:
        n['raw'] = data['records'][n['id']]
        n['title'] = n['raw']['title']; n['kind'] = n['raw']['kind']
        n.setdefault('label', n['title'])
    for e in data['graph']['edges']:
        e['raw'] = data['records'][e['id']]
    if 'projectId' in data:
        data['project'] = data['records'][data['projectId']]
    return data
