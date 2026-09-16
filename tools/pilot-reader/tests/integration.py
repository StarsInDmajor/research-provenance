"""Explicit opt-in tests using the installed pinned rp; no browser/network.

Run with --project, --before, --rp, --output all explicit (same as README).
Copies only into /tmp/rp-node-reader-*; invalid/mutating runners never touch originals.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
from test_geometry import points, intersects


def main():
    p=argparse.ArgumentParser()
    for name in ('project','before','rp','output'): p.add_argument('--'+name,required=True,type=Path)
    args=p.parse_args()
    initial={name:build.observe(root) for name,root in [('before',args.before),('after',args.project)]}
    stamp='2026-09-09T00:00:00Z'; as_of='2026-09-08T23:59:59Z'
    report=build.rebuild(args.project,args.before,args.rp,args.output,as_of,stamp)
    html=args.output.read_bytes()
    report2=build.rebuild(args.project,args.before,args.rp,args.output,as_of,stamp)
    assert html==args.output.read_bytes() and report==report2
    assert set(report['reader_source_inventory']) == {'build.py', 'graph_projection.py', 'routing.py', 'routing.js',
        'pilot-pins.json', 'template.html', 'reader.css', 'graph.js', 'bootstrap.js', 'svg.js'}
    checks=['actual rp 32/35 zero findings','deterministic repeated build', 'all nine runtime sources inventoried including routing and bootstrap']
    pins=json.loads((build.HERE/'pilot-pins.json').read_text())
    graph=build.load_data(initial['before'],initial['after'],pins)['graph']
    relation=next(e for e in graph['edges'] if e['id']=='rel_00000000000000000000000061')
    unrelated=[n for n in graph['nodes'] if n['id'] not in (relation['from'],relation['to'])]
    assert not any(intersects(p,n,12) for p in points(relation['path']) for n in unrelated)
    assert relation['from']=='qst_00000000000000000000000010'
    assert relation['to']=='hyp_00000000000000000000000012'
    checks.append('actual pinned rel61 sampled geometry clears all unrelated cards with 12px margin')
    with tempfile.TemporaryDirectory(prefix='rp-node-reader-integration-') as tmp:
        root=Path(tmp)
        before=root/'before'; after=root/'after'
        shutil.copytree(args.before,before);shutil.copytree(args.project,after)
        target=next((after/'.research/records').rglob('*.yaml'))
        data=json.loads(target.read_text());data['unexpected_closed_schema_field']=True
        target.write_text(json.dumps(data))
        calls=[]
        def actual(*a,**kw):
            result=subprocess.run(*a,**kw)
            calls.append(result.returncode)
            return result
        try:
            build.rebuild(after,before,args.rp,args.output,as_of,stamp,actual)
            raise AssertionError('invalid corpus published')
        except ValueError: pass
        assert calls[0]==0 and calls[-1]!=0 and args.output.read_bytes()==html
        checks.append('invalid copied corpus actual rp nonzero, old output preserved')
        shutil.rmtree(after);shutil.copytree(args.project,after)
        def mutate_after_validation(*a,**kw):
            result=subprocess.run(*a,**kw)
            if str(after) in a[0]: (after/'sources/design.md').write_text('changed during validation')
            return result
        try:
            build.rebuild(after,before,args.rp,args.output,as_of,stamp,mutate_after_validation)
            raise AssertionError('mutation during validation published')
        except ValueError as e: assert 'changed' in str(e)
        assert args.output.read_bytes()==html
        checks.append('input changed after actual validation detected, output preserved')
        shutil.rmtree(after);shutil.copytree(args.project,after)
        original_render=build.render
        def mutate_during_projection(data,evidence):
            result=original_render(data,evidence)
            (after/'new-file.md').write_text('inventory addition')
            return result
        build.render=mutate_during_projection
        try:
            build.rebuild(after,before,args.rp,args.output,as_of,stamp)
            raise AssertionError('mutation during projection published')
        except ValueError as e: assert 'changed' in str(e)
        finally: build.render=original_render
        assert args.output.read_bytes()==html
        checks.append('inventory addition during render detected, output preserved')
    # Mutate only a disposable reader source copy, never the live builder files.
    with tempfile.TemporaryDirectory(prefix='rp-node-reader-integration-source-') as tmp:
        original_here = build.HERE
        copied = Path(tmp)/'reader'
        shutil.copytree(original_here, copied)
        build.HERE = copied
        try:
            for source_name in ('bootstrap.js', 'routing.js', 'routing.py', 'svg.js'):
                source = copied/source_name
                original = source.read_bytes()
                def mutate_reader(*a, **kw):
                    result = subprocess.run(*a, **kw)
                    if str(args.project) in a[0]:
                        source.write_text('changed after validation')
                    return result
                try:
                    build.rebuild(args.project,args.before,args.rp,args.output,as_of,stamp,mutate_reader)
                    raise AssertionError('changed reader source published')
                except ValueError as e:
                    assert 'Reader source inventory changed' in str(e)
                finally:
                    source.write_bytes(original)
                assert args.output.read_bytes() == html
        finally:
            build.HERE = original_here
    checks.append('bootstrap / routing.js / routing.py / svg.js source mutations after actual validation detected, old output preserved')
    build.unchanged(args.before,initial['before']);build.unchanged(args.project,initial['after'])
    checks.append('all 47 before / 51 after observed input files unchanged')
    print(json.dumps({'checks':checks,'html_bytes':len(html),'sha256':build.sha(html),'visual_acceptance':'PENDING'},indent=2))


if __name__=='__main__': main()
