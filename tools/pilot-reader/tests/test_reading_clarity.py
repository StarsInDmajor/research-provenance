"""Display-only regressions; synthetic graphs are not browser evidence."""
import copy
import re
import sys
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
import graph_projection as gp
import lookup
from test_projection import node, edge, binding


class ReadingClarityTests(unittest.TestCase):
    def test_guide_explains_actual_kinds_and_distinguishes_reading_numbers(self):
        q=node('qst_abc');q['title']='R01 Research question'
        m=node('mth_def');m['kind']='Method';m['title']='R02 Model'
        g=gp.project([q,m],'t')
        guide=build.reading_guide(g)
        for value in ('节点分类','Question','问题','qst_','Method','方法','mth_',
                      'R01','阅读编号','不是节点类型'):
            self.assertIn(value,guide)
        self.assertNotIn('Conclusion',guide)
        g['nodes'][0]['kind']='<unknown>'
        self.assertNotIn('<unknown>',build.reading_guide(g))
        self.assertIn('&lt;unknown&gt;',build.reading_guide(g))

    def test_svg_two_line_fit_clipping_full_accessible_text_all_revisions(self):
        for label in ['长中文节点说明'*25, 'WideLatinWWW_Exact_ID_0123456789'*12, '短标签']:
            records=[node('old'),node('new',['old'],'old'), binding('b','new','角色'*100)]
            g=gp.project(records,'t',dict(old=label,new=label))
            svg=ET.fromstring(build.render_svg(g))
            clips=svg.findall('.//clipPath')
            self.assertEqual(len(clips),2)
            self.assertEqual(len({c.attrib['id'] for c in clips}),2)
            for group in [e for e in svg.findall('g') if 'node' in e.attrib['class'].split()]:
                texts=group.findall('.//text')
                title=group.find('title').text
                self.assertIn(label,title)
                self.assertIn(label,group.attrib['aria-label'])
                lines=[t for t in texts if t.attrib.get('class')=='node-label']
                self.assertEqual(len(lines),1 if label=='短标签' else 2)
                self.assertEqual([t.attrib['y'] for t in lines],['50'] if len(lines)==1 else ['50','70'])
                if len(lines)==2:self.assertTrue(lines[-1].text.endswith('…'))
                self.assertTrue(any('clip-path' in el.attrib for el in group.iter()))
                self.assertNotIn('textLength',ET.tostring(group,encoding='unicode'))
                for t in texts:
                    size={'node-type':12,'node-label':15,'node-role':10,'node-status':9}[t.attrib['class']]
                    # Conservative bound independent of production wrap helper.
                    width=sum(size*(.8 if ord(c)<128 else 1.1) for c in t.text or '')
                    self.assertLessEqual(width,238.01)
            self.assertEqual(g['nodes'][0]['label'],label,'projection must keep full labels')

    def test_type_colors_patterns_markers_unknown_injection_and_historical_state(self):
        expected={'supports':('#287344',None),'weakens':('#b23838','3 3'),
                  'contradicts':('#b23838','10 3 2 3'),'derived-from':('#246ca6','8 4'),
                  'depends-on':('#956400','12 3'),'blocked-by':('#b23838','7 4'),
                  'motivates':('#147d79','2 4'),'revision':('#80529b','9 4 2 4')}
        records=[node('a'),node('b',['a'],'a')]
        for i,typ in enumerate([*expected,'evil\" onclick=\"bad']):
            if typ=='revision':continue
            e=edge('e'+str(i),'a','b',state='invalidated' if i==0 else 'active');e['type']=typ;records.append(e)
        g=gp.project(records,'t'); svg=ET.fromstring(build.render_svg(g))
        markers={m.attrib['id']:m.find('path').attrib['fill'] for m in svg.findall('.//marker')}
        by={e['id']:e for e in g['edges']+g['revisionEdges']}
        for group in [e for e in svg.findall('g') if 'edge' in e.attrib['class'].split()]:
            e=by[group.attrib['data-key']]; path=next(p for p in group.findall('path') if p.attrib['class']=='edge-line')
            color,dash=expected.get(e['type'],('#66717b',None))
            self.assertEqual(path.attrib.get('stroke'),color)
            self.assertEqual(path.attrib.get('stroke-dasharray'),dash)
            marker=re.fullmatch(r'url\(#([a-z-]+)\)',path.attrib['marker-end'])[1]
            self.assertEqual(markers[marker],color)
            self.assertNotIn('evil',group.attrib['class'])
            self.assertEqual(group.attrib['data-from'],e['from']);self.assertEqual(group.attrib['data-to'],e['to'])
        self.assertIn('已撤销',build.render_svg(g))
        self.assertEqual(next(e for e in g['edges'] if e['type']=='derived-from')['label'],'推导自/依据')

    def test_shared_known_exact_display_name_lookup_and_original_untouched(self):
        rid='obs_00000000000000000000009104'
        r=node(rid,kind='Observation');r.update(title='I01 有实现不等于可用',logical_id='csp-i01')
        names=[dict(alias='I01',id=rid,name='有实现不等于可用')]
        data=dict(records={rid:r},names=names,manifest=dict(case_id='csp-writing-r2',thread='t'))
        original=copy.deepcopy(data)
        labels=gp.presentation_labels(data['records'],names,'csp-writing-r2')
        self.assertEqual(labels[rid],'I01 历史交互测试通过，浏览器执行未验证')
        data['graph']=gp.project([r],'t',labels)
        for name in [labels[rid],labels[rid].split(' ',1)[1],'I01',r['title']]:
            self.assertEqual(lookup.select(data,name).get('selected_id'),rid)
        del data['graph'];self.assertEqual(data,original)
        self.assertNotIn('历史交互测试通过',gp.presentation_labels(data['records'],names,'other')[rid])
        r['id']='different';data['records']={r['id']:r};names[0]['id']=r['id']
        self.assertNotIn('历史交互测试通过',gp.presentation_labels(data['records'],names,'csp-writing-r2')[r['id']])

    def test_generic_entry_only_current_questions_in_selected_thread(self):
        records=[node('old'),node('new',['old'],'old'),node('other'),node('unbound'),
                 binding('b','new','primary'),binding('c','other','alternative'),binding('d','unbound','primary',thread='elsewhere')]
        g=gp.project(records,'t')
        self.assertEqual(g.get('startIds'),['new','other'])
        self.assertNotIn('aliasLegend',g)

    def test_unknown_science_cannot_impersonate_revision_or_neutral_reserved_keys(self):
        for typ in ['revision','neutral']:
            e=edge('e','a','b');e['type']=typ
            g=gp.project([node('a'),node('b'),e],'t')
            self.assertEqual(g['edges'][0]['label'],typ)
            self.assertEqual(gp.edge_style(g['edges'][0])[0],'neutral')

    def test_case_entry_source_and_legend_are_derived_without_science_changes(self):
        records=[node('q'),node('fault',kind='Observation'),node('method',kind='Method'),
                 binding('b','q','primary')]
        records[0]['source']={'revisions':['fault']}
        names=[dict(alias='Q00',id='q',name='问题'),dict(alias='I02',id='fault',name='故障'),
               dict(alias='I06',id='method',name='方法')]
        g=gp.project(records,'t');original=copy.deepcopy(g)
        gp.case_reading(g,names,'csp-writing-r2')
        self.assertEqual(g['startIds'],['q'])
        self.assertEqual(g['edges'],original['edges']);self.assertEqual(g['nodes'],original['nodes'])
        self.assertIn('SOURCE',g['readingSources']['q']['fault'])
        self.assertIn('I06 = Method',' '.join(g['aliasLegend']))
        self.assertNotIn('I03',' '.join(g['aliasLegend']))
        del records[0]['source']
        g=gp.project(records,'t');gp.case_reading(g,names,'csp-writing-r2')
        self.assertNotIn('readingSources',g,'no invented source when citation absent')

    def test_two_lines_without_ellipsis_and_safe_clip_references(self):
        n=node('a\"/><script>bad</script>');n['title']='中文'*9
        g=gp.project([n],'t');svg=ET.fromstring(build.render_svg(g))
        group=svg.find('g');clip=svg.find('.//clipPath');box=clip.find('rect')
        self.assertEqual(group.find('g').attrib['clip-path'],'url(#'+clip.attrib['id']+')')
        self.assertLessEqual(float(box.attrib['x'])+float(box.attrib['width']),270)
        self.assertLessEqual(float(box.attrib['y'])+float(box.attrib['height']),114)
        lines=[t.text for t in group.findall('.//text') if t.attrib.get('class')=='node-label']
        self.assertEqual(len(lines),2);self.assertEqual(''.join(lines),n['title'])
        self.assertEqual(svg.findall('.//script'),[])
        self.assertRegex(clip.attrib['id'],r'^node-clip-[0-9]+$')

    def test_selection_and_focus_css_does_not_overwrite_type_color_or_pattern(self):
        css=(build.HERE/'reader.css').read_text()
        for selectors,body in re.findall(r'([^{}]+)\{([^{}]*)\}',css):
            if '.edge' in selectors and any(s in selectors for s in ['selected','neighbor',':focus-visible']):
                self.assertNotRegex(body,r'(?:^|;)\s*stroke\s*:')
                self.assertNotIn('stroke-dasharray:',body)


if __name__=='__main__':unittest.main()
