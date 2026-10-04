#!/usr/bin/env python3
"""Native structural checks only. This does not execute JavaScript or a DOM."""
import base64
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import sys

class Package(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.elements = []
        self.scripts = []
        self.styles = []
        self.capture = None
        self.owner = None
        self.owners = []
        self.roots = []
    def handle_starttag(self, tag, attrs):
        keys = [k for k, _ in attrs]
        assert len(keys) == len(set(keys)), f'duplicate attributes on {tag}'
        attrs = dict(attrs)
        assert not any(k.startswith('on') or k == 'style' for k in attrs), 'inline event/style attribute'
        assert not any(k in attrs for k in ['src', 'href', 'xlink:href']), 'external/navigation reference'
        assert tag not in ['iframe', 'object', 'embed', 'foreignobject', 'img', 'link', 'base', 'form'], tag
        if 'data-vizir-explorer' in attrs:
            assert tag == 'section' and attrs['data-vizir-explorer'] == '1'
            assert self.owner is None, 'nested generated roots'
            self.owner = attrs['data-vizir-instance']
            self.roots.append(self.owner)
        self.elements.append((tag, attrs))
        self.owners.append(self.owner)
        if tag in ['script', 'style']:
            assert self.capture is None
            self.capture = [tag, attrs, '']
    def handle_data(self, data):
        if self.capture is not None:
            self.capture[2] += data
    def handle_endtag(self, tag):
        if tag == 'section': self.owner = None
        if self.capture is not None and self.capture[0] == tag:
            (self.scripts if tag == 'script' else self.styles).append(self.capture)
            self.capture = None

def unique(pairs):
    out = {}
    for key, value in pairs:
        assert key not in out, f'duplicate JSON key {key}'
        out[key] = value
    return out

def digest(text):
    return base64.b64encode(hashlib.sha256(text.encode()).digest()).decode()

def check(path):
    raw = Path(path).read_bytes()
    assert len(raw) <= 32 * 1024 * 1024
    parser = Package()
    parser.feed(raw.decode())
    parser.close()
    assert parser.capture is None
    assert len(parser.styles) == 1
    modules = [s for s in parser.scripts if s[1].get('type') == 'module']
    data = [s for s in parser.scripts if s[1].get('type') == 'application/json']
    assert len(modules) == 1 and len(data) >= 1 and len(parser.scripts) == len(data) + 1
    assert len(data) == len(parser.roots) == len(set(parser.roots)), 'duplicate/missing generated root'
    contexts = []
    for entry in data:
        assert '<' not in entry[2] and '\u2028' not in entry[2] and '\u2029' not in entry[2]
        context = json.loads(entry[2], object_pairs_hook=unique)
        assert (context['format'], context['profile']) in [('vizir-interaction/1', 'explorer-v1'), ('vizir-interaction/2', 'explorer-linked-v1')]
        assert context['instance_key'] in parser.roots
        if context['format'] == 'vizir-interaction/2':
            positions = {n['scene_node_id']: i for i, n in enumerate(context['nodes'])}
            groups = context['link_groups']
            assert 1 <= len(groups) <= 256
            assert [g['id'] for g in groups] == sorted(set(g['id'] for g in groups))
            seen = set()
            for group in groups:
                assert 2 <= len(group['members']) <= 256
                indices = [positions[m] for m in group['members']]
                assert indices == sorted(set(indices))
                assert not (seen & set(group['members']))
                seen.update(group['members'])
            assert len(seen) <= 4096
        else:
            assert 'link_groups' not in context
        contexts.append(context)
    assert [c['instance_key'] for c in contexts] == parser.roots
    prefix, payload = modules[0][2].split('\n', 1)
    payload_hash = hashlib.sha256(payload.encode()).hexdigest()
    assert prefix == f'const VIZIR_RUNTIME_PAYLOAD_SHA256 = "{payload_hash}";'
    assert all(c['runtime_payload_sha256'] == payload_hash for c in contexts)
    csp = [a['content'] for t, a in parser.elements if t == 'meta' and a.get('http-equiv') == 'Content-Security-Policy']
    assert len(csp) == 1
    expected = f"default-src 'none'; script-src 'sha256-{digest(modules[0][2])}'; style-src 'sha256-{digest(parser.styles[0][2])}'; connect-src 'none'; img-src 'none'; font-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"
    assert csp[0] == expected, 'CSP hash/policy mismatch'
    ids = [a['id'] for _, a in parser.elements if 'id' in a]
    assert len(ids) == len(set(ids)), 'duplicate DOM IDs'
    refs = []
    for _, attrs in parser.elements:
        refs += attrs.get('aria-labelledby', '').split()
        if 'for' in attrs: refs.append(attrs['for'])
        if 'marker-end' in attrs:
            value = attrs['marker-end']
            assert value.startswith('url(#') and value.endswith(')')
            refs.append(value[5:-1])
    assert all(r in ids for r in refs), 'broken internal reference'
    total_nodes = 0
    for context in contexts:
        emitted = [a for (tag, a), owner in zip(parser.elements, parser.owners) if 'data-vizir-scene-id' in a and owner == context['instance_key']]
        assert len(emitted) == len(context['nodes'])
        total_nodes += len(emitted)
        for attrs, node in zip(emitted, context['nodes']):
            assert attrs['data-vizir-scene-id'] == node['scene_node_id']
            assert attrs['id'] == node['dom_id']
            origin = node['origin']
            assert attrs['data-hir-node'] == origin['hir_node']
            assert attrs['data-mir-node'] == origin['mir_node']
            assert attrs['data-generated-by'] == origin['generated_by']
            assert attrs.get('data-key') == origin.get('data_key')
            assert attrs.get('data-lineage', '') == ','.join(origin.get('data_lineage', []))
    assert total_nodes == sum('data-vizir-scene-id' in a for _, a in parser.elements)
    return {'structural_package_check': 'passed', 'bytes': len(raw), 'nodes': total_nodes, 'instances': len(contexts), 'scene_sha256': contexts[0]['scene_sha256'] if len(contexts) == 1 else None, 'browser_dom_acceptance': 'not_run'}

if __name__ == '__main__':
    for file in sys.argv[1:]:
        print(json.dumps({'file': file, **check(file)}, sort_keys=True))
