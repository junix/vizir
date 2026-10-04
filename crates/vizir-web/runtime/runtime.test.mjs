/** Native tests of metadata and the pure reducer only.
 * These are not DOM, browser, accessibility, CSP enforcement or pointer-capture
 * acceptance tests. Browser acceptance is explicitly BLOCKED / NOT RUN.
 */
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { validateContext, createState, reduce, nodeDomId, nodeReference, viewportKeyAction, validateRuntimeRevision, expectedOriginAttributes, selectionInfo, bootstrap } from './runtime.mjs';

function wire({ ids = ['root', 'child', 'other'], key = 'main', parents = [null, 'root', null] } = {}) {
  return {
    format: 'vizir-interaction/1', profile: 'explorer-v1', document_id: 'example',
    scene_sha256: '0123456789abcdef'.repeat(4), runtime_payload_sha256: 'abcdef0123456789'.repeat(4), instance_key: key,
    home: { x: 0, y: 0, width: 1000, height: 500 },
    camera: { min_zoom: 1, max_zoom: 16, zoom_factor: 1.25, pan_fraction: 0.1, drag_threshold_css_px: 4 },
    nodes: ids.map((id, i) => ({ scene_node_id: id, parent_scene_node_id: parents[i] ?? null, dom_id: nodeDomId(key, id), origin: { hir_node: `hir:${i}`, mir_node: `mir:${i}`, generated_by: 'test', explanation: '' } })),
  };
}
const fixture = () => validateContext(wire());
function invalid(change, pattern = /VizIR explorer:/) {
  const value = wire(); change(value); assert.throws(() => validateContext(value), pattern);
}
function apply(state, context, type, fields = {}) { return reduce(state, { type, ...fields }, context); }
function assertCamera(state, context) {
  const c = state.camera;
  assert.ok(Object.values(c).every(Number.isFinite));
  assert.ok(c.zoom >= 1 && c.zoom <= 16);
  assert.ok(c.x >= 0 && c.y >= 0);
  assert.ok(c.x + c.width <= context.home.width + 1e-9);
  assert.ok(c.y + c.height <= context.home.height + 1e-9);
  assert.equal(c.width, context.home.width / c.zoom);
  assert.equal(c.height, context.home.height / c.zoom);
}
function startDrag(context, state = createState(context)) {
  state = apply(state, context, 'ZoomAt', { factor: 2 });
  state = apply(state, context, 'TogglePan');
  return apply(state, context, 'PointerStart', { pointer_id: 7, css_x: 100, css_y: 100, scene_x: 500, scene_y: 250 });
}
function move(state, context, fields = {}) {
  return apply(state, context, 'PointerMove', { pointer_id: 7, css_x: 120, css_y: 110, scene_x: 540, scene_y: 270, ...fields });
}

test('valid context is copied, normalized, deeply frozen, and idempotent', () => {
  const source = wire(); const before = JSON.stringify(source); const context = validateContext(source);
  assert.notEqual(context, source);
  assert.deepEqual(context.nodes[0].origin.data_lineage, []);
  assert.equal(JSON.stringify(source), before);
  assert.equal(validateContext(context), context);
  for (const object of [context, context.home, context.camera, context.nodes, context.nodes[0], context.nodes[0].origin, context.nodes[0].origin.data_lineage]) assert.ok(Object.isFrozen(object));
  source.home.width = 77; source.nodes[0].origin.explanation = 'changed';
  assert.equal(context.home.width, 1000); assert.equal(context.nodes[0].origin.explanation, '');
});

test('wire versions, profiles, unknown and missing fields fail closed', () => {
  for (const field of ['format', 'profile', 'document_id', 'scene_sha256', 'runtime_payload_sha256', 'instance_key', 'home', 'camera', 'nodes']) invalid((v) => { delete v[field]; });
  invalid((v) => { v.format = 'vizir-interaction/2'; });
  invalid((v) => { v.profile = 'future'; });
  for (const target of [(v) => v, (v) => v.home, (v) => v.camera, (v) => v.nodes[0], (v) => v.nodes[0].origin]) invalid((v) => { target(v).unrecognized = true; });
});

test('metadata rejects prototypes, symbols, nonenumerable fields and getters without invoking them', () => {
  invalid((v) => { Object.setPrototypeOf(v.home, { x: 0 }); });
  invalid((v) => { v[Symbol('extra')] = 1; });
  invalid((v) => { Object.defineProperty(v.home, 'x', { enumerable: false }); });
  let calls = 0;
  invalid((v) => { Object.defineProperty(v, 'document_id', { enumerable: true, get() { calls += 1; return 'x'; } }); });
  assert.equal(calls, 0);
  invalid((v) => { Object.defineProperty(v, '__proto__', { value: {}, enumerable: true }); });
  assert.equal({}.polluted, undefined);
  const nullProto = wire(); Object.setPrototypeOf(nullProto, null); assert.equal(validateContext(nullProto).document_id, 'example');
});

test('arrays reject holes, accessors, subclasses and custom properties', () => {
  invalid((v) => { delete v.nodes[0]; });
  invalid((v) => { v.nodes.extra = 1; });
  invalid((v) => { v.nodes[Symbol('extra')] = 1; });
  invalid((v) => { Object.setPrototypeOf(v.nodes, null); });
  let calls = 0;
  invalid((v) => { Object.defineProperty(v.nodes, '0', { enumerable: true, get() { calls += 1; return {}; } }); });
  assert.equal(calls, 0);
});

test('digest and instance-key grammar is strict', () => {
  for (const digest of ['', 'a'.repeat(63), 'a'.repeat(65), 'A'.repeat(64), 'g'.repeat(64), null, 12]) invalid((v) => { v.scene_sha256 = digest; });
  for (const key of ['', '1abc', 'Upper', 'a_b', 'a:b', 'a b', 'é', 'a'.repeat(33), null]) invalid((v) => { v.instance_key = key; });
  assert.equal(validateContext(wire({ ids: [], key: 'a'.repeat(32) })).instance_key.length, 32);
});

test('required identities match Rust Unicode White_Space semantics', () => {
  for (const blank of ['', ' ', '\t\n', '\u0085', '\u2003', '\u3000']) {
    invalid((v) => { v.document_id = blank; });
    invalid((v) => { v.nodes[0].origin.hir_node = blank; });
  }
  const value = wire({ ids: ['\ufeff'], parents: [] }); value.document_id = '\ufeff';
  value.nodes[0].origin.generated_by = '\ufeff';
  assert.equal(validateContext(value).nodes[0].scene_node_id, '\ufeff');
});

test('optional data strings preserve emptiness, whitespace and punctuation exactly', () => {
  const value = wire(); value.nodes[0].origin.data_key = '';
  value.nodes[0].origin.data_lineage = ['', ' ', 'a,b', 'a', 'b', '__proto__'];
  assert.deepEqual(validateContext(value).nodes[0].origin.data_lineage, value.nodes[0].origin.data_lineage);
  assert.equal(validateContext(value).nodes[0].origin.data_key, '');
  for (const entry of [undefined, true, 10, {}, []]) invalid((v) => { v.nodes[0].origin.data_key = entry; });
  invalid((v) => { v.nodes[0].origin.data_lineage = [null]; });
  invalid((v) => { v.nodes[0].origin.data_lineage = 'a,b'; });
});

test('strings enforce UTF8 byte limits and reject lone surrogates', () => {
  const value = wire(); value.nodes[0].origin.explanation = '😀'.repeat(1024);
  assert.equal(validateContext(value).nodes[0].origin.explanation.length, 2048);
  invalid((v) => { v.document_id = '😀'.repeat(1025); });
  invalid((v) => { v.nodes[0].origin.explanation = 'a'.repeat(4097); });
  for (const unpaired of ['\ud800', '\udfff', 'ok\ud800x']) invalid((v) => { v.document_id = unpaired; });
});

test('derived hex DOM IDs are injective and may exceed source string limits', () => {
  const ids = ['a:b', 'a b', 'a-b', 'a_b', 'é', 'e\u0301', '😀', '__proto__', 'constructor', 'toString', '#x"] > script', '<script>alert(1)</script>'];
  const context = validateContext(wire({ ids, parents: [] }));
  assert.equal(new Set(context.nodes.map((node) => node.dom_id)).size, ids.length);
  assert.equal(nodeDomId('main', 'é'), 'vzi-main-node-c3a9');
  assert.equal(nodeDomId('main', '😀'), 'vzi-main-node-f09f9880');
  const large = validateContext(wire({ ids: ['😀'.repeat(1024)], parents: [] }));
  assert.ok(large.nodes[0].dom_id.length > 8192);
  invalid((v) => { v.nodes[0].dom_id = v.nodes[0].scene_node_id; });
  invalid((v) => { v.nodes[1].dom_id = v.nodes[0].dom_id; });
});

test('identities are Map-safe and references carry only snapshot identity', () => {
  const context = validateContext(wire({ ids: ['__proto__', 'constructor', 'toString'], parents: [] }));
  for (const node of context.nodes) {
    const ref = nodeReference(context, node.scene_node_id);
    assert.deepEqual(Object.keys(ref).sort(), ['document_id', 'instance_key', 'scene_node_id', 'scene_sha256']);
    assert.equal(ref.scene_node_id, node.scene_node_id); assert.ok(Object.isFrozen(ref));
  }
  assert.equal(nodeReference(context, 'absent'), null);
});

test('node uniqueness, preceding parents, preorder and depth bounds', () => {
  invalid((v) => { v.nodes[1] = structuredClone(v.nodes[0]); });
  invalid((v) => { v.nodes[0].parent_scene_node_id = 'child'; });
  invalid((v) => { v.nodes[1].parent_scene_node_id = 'child'; });
  invalid((v) => { v.nodes[1].parent_scene_node_id = 'missing'; });
  const interrupted = wire({ ids: ['a', 'b', 'c', 'd'], parents: [null, 'a', null, 'a'] });
  assert.throws(() => validateContext(interrupted), /preorder/);
  const chain = (n) => wire({ ids: Array.from({ length: n }, (_, i) => `n${i}`), parents: Array.from({ length: n }, (_, i) => i ? `n${i - 1}` : null) });
  assert.equal(validateContext(chain(64)).nodes.length, 64);
  assert.throws(() => validateContext(chain(65)), /depth/);
  const siblings = wire({ ids: ['a', 'b', 'c', 'd', 'e'], parents: [null, 'a', 'b', 'a', null] });
  assert.equal(validateContext(siblings).nodes.length, 5);
});

test('node, lineage and aggregate metadata limits are enforced', () => {
  assert.equal(validateContext(wire({ ids: Array.from({ length: 8192 }, (_, i) => `n${i}`), parents: [] })).nodes.length, 8192);
  assert.throws(() => validateContext(wire({ ids: Array.from({ length: 8193 }, (_, i) => `n${i}`), parents: [] })), /array/);
  const lineage = wire(); lineage.nodes[0].origin.data_lineage = Array(32768).fill('');
  assert.equal(validateContext(lineage).nodes[0].origin.data_lineage.length, 32768);
  lineage.nodes[1].origin.data_lineage = [''];
  assert.throws(() => validateContext(lineage), /lineage/);
  const giant = wire({ ids: Array.from({ length: 1024 }, (_, i) => `n${i}`), parents: [] });
  for (const node of giant.nodes) node.origin.explanation = 'x'.repeat(4096);
  assert.throws(() => validateContext(giant), /byte limit/);
});

test('home and fixed camera policies reject nonfinite, wrong-type and unsupported bounds', () => {
  for (const size of [0, -1, 0.5, 1e6 + 1, NaN, Infinity, -Infinity, '10', null]) invalid((v) => { v.home.width = size; });
  for (const position of [1, -1, NaN, Infinity, '0']) invalid((v) => { v.home.x = position; });
  for (const field of Object.keys(wire().camera)) invalid((v) => { v.camera[field] += 0.1; });
  const small = wire(); small.home.width = 1; small.home.height = 1e6;
  assert.equal(validateContext(small).home.width, 1);
});

test('state starts at home with no selection, gesture or pan', () => {
  const context = fixture(); const state = createState(context);
  assert.deepEqual(state.camera, { ...context.home, zoom: 1 });
  assert.equal(state.selected, null); assert.equal(state.hovered, null); assert.equal(state.focused, null);
  assert.equal(state.pan_mode, false); assert.equal(state.gesture, null); assert.equal(state.suppress_click, false);
  assert.ok(Object.isFrozen(state)); assert.ok(Object.isFrozen(state.camera));
});

test('state provenance rejects fabricated, cloned, malformed and foreign snapshots', () => {
  const context = fixture(); const state = createState(context);
  for (const forged of [null, {}, structuredClone(state), { ...state, camera: { x: NaN } }, { ...state, gesture: { pointer_id: 1 } }]) assert.throws(() => apply(forged, context, 'Reset'), /state belongs/);
  const other = wire(); other.scene_sha256 = 'f'.repeat(64);
  assert.throws(() => apply(state, validateContext(other), 'Reset'), /state belongs/);
  assert.throws(() => apply(state, validateContext(wire({ key: 'other' })), 'Reset'), /state belongs/);
});

test('center zoom preserves aspect, clamps1..16, and handles extreme finite factors', () => {
  const context = fixture(); let state = createState(context);
  state = apply(state, context, 'ZoomAt', { factor: 2 });
  assert.deepEqual(state.camera, { x: 250, y: 125, width: 500, height: 250, zoom: 2 });
  state = apply(state, context, 'ZoomAt', { factor: Number.MAX_VALUE }); assert.equal(state.camera.zoom, 16); assertCamera(state, context);
  state = apply(state, context, 'ZoomAt', { factor: Number.MIN_VALUE }); assert.equal(state.camera.zoom, 1); assert.deepEqual(state.camera, { ...context.home, zoom: 1 });
  for (const factor of [NaN, Infinity, -Infinity, 0, -1, '2', null]) assert.equal(apply(state, context, 'ZoomAt', { factor }), state);
});

test('zoom anchors preserve in-view point position and clamp out-of-view anchors', () => {
  const context = fixture(); const initial = createState(context);
  const state = apply(initial, context, 'ZoomAt', { factor: 2, x: 200, y: 100 });
  assert.equal((200 - state.camera.x) / state.camera.width, 200 / context.home.width);
  assert.equal((100 - state.camera.y) / state.camera.height, 100 / context.home.height);
  const corner = apply(initial, context, 'ZoomAt', { factor: 2, x: -1e300, y: 1e300 });
  assert.deepEqual(corner.camera, { x: 0, y: 250, width: 500, height: 250, zoom: 2 });
  assert.equal(apply(initial, context, 'ZoomAt', { factor: 2, x: NaN }), initial);
});

test('arrow pan is10% of visible dimensions and never leaves document', () => {
  const context = fixture(); let state = createState(context);
  state = apply(state, context, 'Pan', { dx: -1, dy: 1 }); assert.deepEqual(state.camera, { ...context.home, zoom: 1 });
  state = apply(state, context, 'ZoomAt', { factor: 2 });
  state = apply(state, context, 'Pan', { dx: 1, dy: -1 }); assert.equal(state.camera.x, 300); assert.equal(state.camera.y, 100);
  for (let i = 0; i < 30; i += 1) state = apply(state, context, 'Pan', { dx: Number.MAX_VALUE, dy: -Number.MAX_VALUE });
  assert.equal(state.camera.x, 500); assert.equal(state.camera.y, 0); assertCamera(state, context);
  assert.equal(apply(state, context, 'Pan', { dx: Infinity, dy: 0 }), state);
});

test('reset restores only camera and cancels an active gesture', () => {
  const context = fixture(); let state = createState(context);
  const selected = nodeReference(context, 'child');
  state = apply(state, context, 'Select', { reference: selected });
  state = apply(state, context, 'Hover', { reference: selected });
  state = startDrag(context, state); state = move(state, context);
  state = apply(state, context, 'Reset');
  assert.deepEqual(state.camera, { ...context.home, zoom: 1 }); assert.equal(state.selected, selected); assert.equal(state.hovered, selected);
  assert.equal(state.pan_mode, true); assert.equal(state.gesture, null); assert.equal(state.suppress_click, true);
});

test('exact scene selection is singular and repeated selection is idempotent', () => {
  const context = fixture(); let state = createState(context);
  const child = nodeReference(context, 'child'); const root = nodeReference(context, 'root');
  state = apply(state, context, 'Select', { reference: child }); assert.equal(state.selected, child);
  assert.equal(apply(state, context, 'Select', { reference: { ...child } }), state);
  state = apply(state, context, 'Select', { reference: root }); assert.equal(state.selected, root);
  state = apply(state, context, 'Select', { reference: null }); assert.equal(state.selected, null);
  assert.equal(apply(state, context, 'Select', { reference: null }), state);
});

test('selection never broadens by HIR identity, data key, DOM ID, parent or other instance', () => {
  const context = fixture(); const initial = createState(context);
  const ref = nodeReference(context, 'child');
  for (const value of ['child', { scene_node_id: 'child' }, { ...ref, scene_node_id: 'hir:1' }, { ...ref, scene_node_id: context.nodes[1].dom_id }, { ...ref, instance_key: 'other' }, { ...ref, document_id: 'other' }, { ...ref, scene_sha256: 'f'.repeat(64) }]) assert.equal(apply(initial, context, 'Select', { reference: value }), initial);
  const selected = apply(initial, context, 'Select', { reference: { ...ref, dom_id: 'ignored' } });
  assert.deepEqual(Object.keys(selected.selected).sort(), ['document_id', 'instance_key', 'scene_node_id', 'scene_sha256']);
});

test('hover and focus inspect without changing selection or each other', () => {
  const context = fixture(); const root = nodeReference(context, 'root'); const child = nodeReference(context, 'child'); let state = createState(context);
  state = apply(state, context, 'Select', { reference: root }); state = apply(state, context, 'Hover', { reference: child }); state = apply(state, context, 'Focus', { reference: root });
  assert.equal(state.selected, root); assert.equal(state.hovered, child); assert.equal(state.focused, root);
  assert.equal(apply(state, context, 'Focus', { reference: { ...root } }), state);
  state = apply(state, context, 'Hover', { reference: null }); assert.equal(state.hovered, null); assert.equal(state.focused, root);
});

test('drag requires explicit pan mode, valid primary pointer ID and finite normalized coordinates', () => {
  const context = fixture(); const initial = createState(context);
  const fields = { pointer_id: 7, css_x: 0, css_y: 0, scene_x: 0, scene_y: 0 };
  assert.equal(apply(initial, context, 'PointerStart', fields), initial);
  const pan = apply(initial, context, 'TogglePan');
  for (const invalidFields of [{ pointer_id: -1 }, { pointer_id: 1.5 }, { css_x: NaN }, { scene_y: Infinity }]) assert.equal(apply(pan, context, 'PointerStart', { ...fields, ...invalidFields }), pan);
});

test('drag threshold uses Euclidean CSS distance, not Scene units', () => {
  const context = fixture(); const started = startDrag(context);
  assert.equal(move(started, context, { css_x: 102, css_y: 102, scene_x: 900 }), started);
  const moved = move(started, context, { css_x: 104, css_y: 100, scene_x: 508, scene_y: 250 });
  assert.equal(moved.gesture.dragging, true); assert.equal(moved.camera.x, 242); assert.equal(moved.camera.y, 125);
  assert.ok(Object.isFrozen(moved.gesture));
});

test('pointer movement uses start-camera coordinates with clamped finite output', () => {
  const context = fixture(); const started = startDrag(context); const once = move(started, context);
  assert.deepEqual(once.camera, { x: 210, y: 105, width: 500, height: 250, zoom: 2 });
  const twice = move(once, context, { scene_x: 560, scene_y: 290 }); assert.equal(twice.camera.x, 190); assert.equal(twice.camera.y, 85);
  const edge = move(twice, context, { scene_x: -1e200, scene_y: 1e200 }); assert.equal(edge.camera.x, 500); assert.equal(edge.camera.y, 0); assertCamera(edge, context);
  assert.equal(move(started, context, { pointer_id: 8 }), started);
  assert.equal(move(started, context, { scene_x: NaN }), started);
});

test('pointer end suppresses exactly the trailing drag click, preserving existing selection', () => {
  const context = fixture(); const selected = nodeReference(context, 'root'); let state = apply(createState(context), context, 'Select', { reference: selected });
  state = move(startDrag(context, state), context);
  assert.equal(apply(state, context, 'PointerEnd', { pointer_id: 8 }), state);
  state = apply(state, context, 'PointerEnd', { pointer_id: 7 }); assert.equal(state.gesture, null); assert.equal(state.suppress_click, true);
  state = apply(state, context, 'Click', { reference: nodeReference(context, 'child') }); assert.equal(state.selected, selected); assert.equal(state.suppress_click, false);
  state = apply(state, context, 'Click', { reference: nodeReference(context, 'child') }); assert.equal(state.selected.scene_node_id, 'child');
});

test('below-threshold pan clicks can select and new pointer starts clear stale suppression', () => {
  const context = fixture(); let state = startDrag(context);
  state = apply(state, context, 'PointerEnd', { pointer_id: 7 }); assert.equal(state.suppress_click, false);
  state = apply(state, context, 'Click', { reference: nodeReference(context, 'child') }); assert.equal(state.selected.scene_node_id, 'child');
  state = apply(startDrag(context), context, 'CancelGesture'); assert.equal(state.suppress_click, true);
  state = apply(state, context, 'PrepareClick'); assert.equal(state.suppress_click, false);
});

test('second pointer, cancel, blur/lostcapture normalization, Escape and pan toggle cancel gestures', () => {
  const context = fixture(); const selected = nodeReference(context, 'child');
  const initial = apply(createState(context), context, 'Select', { reference: selected });
  const started = startDrag(context, initial);
  for (const action of [{ type: 'CancelGesture' }, { type: 'Escape' }, { type: 'TogglePan' }, { type: 'PointerStart', pointer_id: 8 }]) {
    const state = reduce(started, action, context); assert.equal(state.gesture, null); assert.equal(state.selected, selected); assert.equal(state.suppress_click, true);
    assert.equal(apply(state, context, 'PointerEnd', { pointer_id: 7 }), state);
  }
  const cancelled = apply(started, context, 'Escape');
  const cleared = apply(cancelled, context, 'Escape'); assert.equal(cleared.selected, null);
});

test('camera commands cancel drags and late moves cannot overwrite their camera', () => {
  const context = fixture(); const started = move(startDrag(context), context);
  for (const action of [{ type: 'ZoomAt', factor: 1.25 }, { type: 'Pan', dx: 1, dy: 0 }, { type: 'Reset' }]) {
    const state = reduce(started, action, context); assert.equal(state.gesture, null); assert.equal(move(state, context), state);
  }
});

test('reducer never mutates metadata, previous state or action and can use raw contexts', () => {
  const raw = wire(); const original = JSON.stringify(raw); const context = validateContext(raw); const state = createState(context); const snapshot = JSON.stringify(state);
  const action = Object.freeze({ type: 'ZoomAt', factor: 2 }); const next = reduce(state, action, context);
  assert.equal(JSON.stringify(state), snapshot); assert.equal(JSON.stringify(raw), original); assert.notEqual(next, state);
  assert.throws(() => { next.camera.x = 9; }, TypeError);
  const reference = nodeReference(raw, 'child'); const selected = reduce(createState(raw), { type: 'Select', reference }, raw);
  assert.equal(reduce(selected, { type: 'Select', reference }, raw), selected);
});

test('distinct instances remain isolated across selection, camera and cancellation', () => {
  const left = fixture(); const right = validateContext(wire({ key: 'right' }));
  let a = createState(left); const b = createState(right); const bBefore = JSON.stringify(b);
  a = apply(a, left, 'Select', { reference: nodeReference(left, 'child') }); a = startDrag(left, a); a = move(a, left); a = apply(a, left, 'CancelGesture');
  assert.equal(JSON.stringify(b), bBefore); assert.equal(b.selected, null); assert.equal(b.camera.zoom, 1);
  assert.equal(apply(b, right, 'Select', { reference: a.selected }), b);
});

test('long deterministic action sequence always retains finite bounded immutable state', () => {
  const context = fixture(); let state = createState(context);
  for (let i = 0; i < 2000; i += 1) {
    state = apply(state, context, 'ZoomAt', { factor: i % 7 ? 1.25 : 0.03125, x: i * 913 - 1000, y: i * -103 });
    state = apply(state, context, 'Pan', { dx: (i % 3) - 1, dy: (i % 5) - 2 });
    if (i % 41 === 0) state = apply(state, context, 'Reset');
    assertCamera(state, context); assert.ok(Object.isFrozen(state));
  }
});

test('unsupported actions fail explicitly rather than silently dropping behavior', () => {
  const context = fixture(); const state = createState(context);
  for (const action of [null, {}, { type: 'Filter' }, { type: 'Animate' }, { type: 'UpdateData' }, { type: 'DomainZoom' }, { type: '__proto__' }]) assert.throws(() => reduce(state, action, context), /action/);
});

test('Node import and bootstrap without document remain DOM-free', () => {
  assert.equal(typeof globalThis.document, 'undefined');
  assert.equal(bootstrap({}), null);
  assert.equal(Object.prototype.hasOwnProperty.call(globalThis, 'VizIRExplorerV1'), false);
});

test('static source audit rejects prohibited APIs; this is not browser acceptance', async () => {
  const source = await readFile(new URL('./runtime.mjs', import.meta.url), 'utf8');
  for (const pattern of [/\bfetch\s*\(/, /\beval\s*\(/, /new\s+Function\b/, /\.innerHTML\s*=/, /\.style\s*[.\[]/, /(?:setTimeout|setInterval|requestAnimationFrame)\s*\(/, /addEventListener\(\s*['"](?:wheel|touchmove)['"]/]) assert.doesNotMatch(source, pattern);
  assert.match(source, /Static visualization remains visible/);
});


test('pure viewport key normalization preserves browser/editor shortcuts and supports shifted plus', () => {
  assert.deepEqual(viewportKeyAction({ key: '+', shiftKey: true }), { type: 'ZoomAt', factor: 1.25 });
  assert.deepEqual(viewportKeyAction({ key: '=' }), { type: 'ZoomAt', factor: 1.25 });
  assert.deepEqual(viewportKeyAction({ key: '-' }), { type: 'ZoomAt', factor: 0.8 });
  assert.deepEqual(viewportKeyAction({ key: 'ArrowLeft' }), { type: 'Pan', dx: -1, dy: 0 });
  assert.deepEqual(viewportKeyAction({ key: 'Home' }), { type: 'Reset' });
  assert.deepEqual(viewportKeyAction({ key: '0' }), { type: 'Reset' });
  for (const key of ['ArrowLeft', 'ArrowRight', 'Home', '0', '+', '-', 'Escape']) {
    for (const modifier of ['ctrlKey', 'altKey', 'metaKey', 'isComposing', 'defaultPrevented']) assert.equal(viewportKeyAction({ key, [modifier]: true }), null);
    if (key !== '+') assert.equal(viewportKeyAction({ key, shiftKey: true }), null);
  }
  for (const key of ['Tab', 'Enter', ' ', 'PageDown', 'f', '__proto__', 'constructor']) assert.equal(viewportKeyAction({ key }), null);
});


test('a reused digest cannot substitute different metadata under a retained state', () => {
  const original = wire(); const context = validateContext(original); const state = createState(context);
  for (const change of [(v) => { v.home.width = 1; }, (v) => { v.nodes = []; }, (v) => { v.nodes[0].origin.explanation = 'other'; }]) {
    const impostor = structuredClone(original); change(impostor);
    assert.throws(() => apply(state, validateContext(impostor), 'Select', { reference: null }), /state belongs/);
  }
  assert.equal(apply(state, validateContext(structuredClone(original)), 'Select', { reference: null }), state);
});


test('payload revision binding rejects invalid and mismatched same-version bundles', () => {
  const revision = 'a'.repeat(64);
  assert.equal(validateRuntimeRevision(revision, revision), revision);
  assert.throws(() => validateRuntimeRevision(revision, 'b'.repeat(64)), /incompatible runtime/);
  for (const value of [null, undefined, '', 'A'.repeat(64), 'a'.repeat(63), 'a'.repeat(65), 'z'.repeat(64)]) {
    assert.throws(() => validateRuntimeRevision(revision, value), /runtime payload/);
    assert.throws(() => validateRuntimeRevision(value, revision), /runtime payload/);
    invalid((v) => { v.runtime_payload_sha256 = value; });
  }
  const original = fixture(); const altered = wire(); altered.runtime_payload_sha256 = 'b'.repeat(64);
  assert.throws(() => apply(createState(original), validateContext(altered), 'Reset'), /state belongs/);
});


test('focus and native-picker intent override stale hover; a new hover still inspects', () => {
  const context = fixture(); const a = nodeReference(context, 'root'); const b = nodeReference(context, 'child');
  let state = apply(createState(context), context, 'Hover', { reference: a });
  assert.equal(state.inspected, a);
  state = apply(state, context, 'Focus', { reference: b });
  assert.equal(state.inspected, b); assert.equal(state.hovered, a);
  state = apply(state, context, 'Select', { reference: b });
  assert.equal(state.inspected, b);
  state = apply(state, context, 'Hover', { reference: a });
  assert.equal(state.inspected, a); assert.equal(state.selected, b);
  state = apply(state, context, 'Select', { reference: b });
  assert.equal(state.inspected, b);
  assert.equal(apply(state, context, 'Select', { reference: b }), state);
  state = apply(state, context, 'Hover', { reference: null }); assert.equal(state.inspected, b);
  state = apply(state, context, 'Focus', { reference: null }); assert.equal(state.inspected, b);
  state = apply(state, context, 'Select', { reference: null }); assert.equal(state.inspected, null);
});

test('instance and digest grammars reject every trailing line terminator', () => {
  for (const ending of ['\n', '\r', '\r\n', '\u2028', '\u2029']) {
    invalid((v) => { v.instance_key = `main${ending}`; });
    invalid((v) => { v.scene_sha256 += ending; });
    invalid((v) => { v.runtime_payload_sha256 += ending; });
    assert.throws(() => validateRuntimeRevision('a'.repeat(64), 'a'.repeat(64) + ending), /runtime payload/);
  }
});


test('explicit null data_key is canonically omitted and expects no DOM attribute', () => {
  const omittedSource = wire(); const nullSource = wire();
  nullSource.nodes[0].origin.data_key = null;
  const omitted = validateContext(omittedSource); const normalizedNull = validateContext(nullSource);
  assert.deepEqual(normalizedNull, omitted);
  assert.equal(Object.hasOwn(normalizedNull.nodes[0].origin, 'data_key'), false);
  assert.equal(nullSource.nodes[0].origin.data_key, null);
  const state = createState(omitted);
  assert.equal(apply(state, normalizedNull, 'Select', { reference: null }), state);
  // Pure attribute projection used by validateDOM: null is getAttribute's absent sentinel.
  // This checks projection semantics, not real browser/DOM acceptance.
  for (const context of [omitted, normalizedNull]) {
    assert.equal(new Map(expectedOriginAttributes(context.nodes[0].origin)).get('data-key'), null);
  }
  const presentEmpty = wire(); presentEmpty.nodes[0].origin.data_key = '';
  assert.equal(new Map(expectedOriginAttributes(validateContext(presentEmpty).nodes[0].origin)).get('data-key'), '');
});

function linkedWire(options = {}) {
  const value = wire(options);
  value.format = 'vizir-interaction/2'; value.profile = 'explorer-linked-v1';
  value.link_groups = options.groups ?? [{ id: 'pair', members: ['root', 'child'] }];
  return value;
}
const linkedFixture = () => validateContext(linkedWire());
function invalidLinked(change, pattern = /VizIR explorer:/) {
  const value = linkedWire(); change(value); assert.throws(() => validateContext(value), pattern);
}
const linkedIds = (state) => state.linked.map((ref) => ref.scene_node_id);

test('v2 metadata is independently copied, canonical, frozen and profile-specific', () => {
  const source = linkedWire(); const before = JSON.stringify(source); const context = validateContext(source);
  assert.equal(context.format, 'vizir-interaction/2'); assert.equal(context.profile, 'explorer-linked-v1');
  assert.deepEqual(context.link_groups, source.link_groups); assert.notEqual(context.link_groups, source.link_groups);
  assert.equal(JSON.stringify(source), before); assert.equal(validateContext(context), context);
  for (const value of [context, context.link_groups, context.link_groups[0], context.link_groups[0].members]) assert.ok(Object.isFrozen(value));
  source.link_groups[0].members[0] = 'other'; source.link_groups[0].id = 'changed';
  assert.deepEqual(context.link_groups, [{ id: 'pair', members: ['root', 'child'] }]);
  const v1 = fixture(); assert.equal(Object.hasOwn(v1, 'link_groups'), false);
});

test('v1 wire stays strict while v2 requires the exact format/profile pair and groups', () => {
  invalid((v) => { v.link_groups = []; }, /unknown or missing/);
  invalid((v) => { v.link_groups = null; }, /unknown or missing/);
  invalid((v) => { v.profile = 'explorer-linked-v1'; }, /unsupported/);
  invalidLinked((v) => { v.profile = 'explorer-v1'; }, /unsupported/);
  invalidLinked((v) => { v.format = 'vizir-interaction/1'; }, /unsupported/);
  invalidLinked((v) => { delete v.link_groups; }, /unknown or missing/);
  invalidLinked((v) => { v.link_groups = null; });
  invalidLinked((v) => { v.link_groups = []; }, /empty/);
  for (const field of ['selected_group_id', 'linked', 'links', 'unknown']) {
    invalid((v) => { v[field] = []; }); invalidLinked((v) => { v[field] = []; });
  }
  for (const field of ['id', 'members']) invalidLinked((v) => { delete v.link_groups[0][field]; });
  invalidLinked((v) => { v.link_groups[0].unknown = true; });
  invalidLinked((v) => { v.link_groups[0] = null; });
});

test('v2 groups and member arrays reject accessors, exotic objects, holes and hidden fields', () => {
  let calls = 0;
  for (const target of [(v) => v.link_groups[0], (v) => v.link_groups]) {
    invalidLinked((v) => { Object.setPrototypeOf(target(v), {}); });
    invalidLinked((v) => { target(v)[Symbol('extra')] = 1; });
  }
  invalidLinked((v) => { Object.defineProperty(v.link_groups[0], 'id', { enumerable: true, get() { calls += 1; return 'pair'; } }); });
  invalidLinked((v) => { Object.defineProperty(v.link_groups[0].members, '0', { enumerable: true, get() { calls += 1; return 'root'; } }); });
  invalidLinked((v) => { Object.defineProperty(v.link_groups[0], 'id', { enumerable: false }); });
  invalidLinked((v) => { delete v.link_groups[0].members[0]; });
  invalidLinked((v) => { v.link_groups[0].members.extra = 'root'; });
  assert.equal(calls, 0);
  const cyclic = linkedWire(); cyclic.link_groups[0].members[0] = cyclic;
  assert.throws(() => validateContext(cyclic), /nesting/);
});

test('v2 group identities have absolute ASCII grammar and 64-byte bound', () => {
  for (const id of ['', 'A', '1a', 'a_b', 'a.b', 'é', 'a b', 'a'.repeat(65), null, 1, 'a\ud800']) invalidLinked((v) => { v.link_groups[0].id = id; });
  for (const ending of ['\n', '\r', '\r\n', '\u2028', '\u2029']) invalidLinked((v) => { v.link_groups[0].id += ending; });
  const value = linkedWire(); value.link_groups[0].id = 'a'.repeat(64);
  assert.equal(validateContext(value).link_groups[0].id.length, 64);
});

test('v2 groups use unique ASCII order and members use exact Scene preorder, not lexical order', () => {
  const value = linkedWire({ ids: ['z', 'a', 'x', 'b'], parents: [], groups: [{ id: 'a', members: ['z', 'a'] }, { id: 'z', members: ['x', 'b'] }] });
  assert.deepEqual(validateContext(value).link_groups, value.link_groups);
  const reversedGroups = structuredClone(value); reversedGroups.link_groups.reverse();
  assert.throws(() => validateContext(reversedGroups), /sorted/);
  const duplicateGroup = structuredClone(value); duplicateGroup.link_groups[1].id = 'a';
  assert.throws(() => validateContext(duplicateGroup), /sorted/);
  invalidLinked((v) => { v.link_groups[0].members.reverse(); }, /preorder/);
  invalidLinked((v) => { v.link_groups[0].members = ['root', 'root']; }, /preorder/);
});

test('v2 rejects missing/foreign member identities and overlapping groups without union', () => {
  for (const member of ['absent', 'hir:0', 'mir:0', nodeDomId('main', 'root'), '', ' ', null, {}, 3, 'root\n', 'root\ud800']) invalidLinked((v) => { v.link_groups[0].members[1] = member; });
  invalidLinked((v) => { v.link_groups = [{ id: 'a', members: ['root', 'child'] }, { id: 'b', members: ['child', 'other'] }]; }, /overlapping/);
  invalidLinked((v) => { v.link_groups[0].members = ['root']; }, /two members/);
  invalidLinked((v) => { v.link_groups[0].members = null; });
  invalidLinked((v) => { v.link_groups[0].members = 'root,child'; });
});

test('v2 group count bounds accept 256 disjoint groups and reject 257', () => {
  const ids = Array.from({ length: 514 }, (_, i) => `n${i}`);
  const groups = Array.from({ length: 257 }, (_, i) => ({ id: `g${String(i).padStart(3, '0')}`, members: ids.slice(i * 2, i * 2 + 2) }));
  const value = linkedWire({ ids, parents: [], groups: groups.slice(0, 256) });
  assert.equal(validateContext(value).link_groups.length, 256);
  value.link_groups = groups; assert.throws(() => validateContext(value), /array/);
});

test('v2 group member bounds accept 256 per group and 4096 total, rejecting either overflow', () => {
  const ids = Array.from({ length: 4098 }, (_, i) => `n${i}`);
  const groups = Array.from({ length: 16 }, (_, i) => ({ id: `g${String(i).padStart(2, '0')}`, members: ids.slice(i * 256, i * 256 + 256) }));
  const value = linkedWire({ ids, parents: [], groups });
  const context = validateContext(value);
  assert.equal(context.link_groups.reduce((n, g) => n + g.members.length, 0), 4096);
  const selected = apply(createState(context), context, 'Select', { reference: nodeReference(context, 'n128') });
  assert.equal(selected.linked.length, 255); assert.equal(selected.linked[0].scene_node_id, 'n0');
  const tooWide = structuredClone(value); tooWide.link_groups[0].members.push(ids[256]);
  assert.throws(() => validateContext(tooWide), /array/);
  value.link_groups.push({ id: 'z', members: ids.slice(4096) });
  assert.throws(() => validateContext(value), /member count/);
  value.link_groups[0].members.pop();
  assert.equal(value.link_groups.reduce((n, g) => n + g.members.length, 0), 4097);
  assert.throws(() => validateContext(value), /member count/);
});

test('v2 source member strings preserve exact Unicode and enforce the existing byte bound', () => {
  const longId = '😀'.repeat(1024);
  const ids = [longId, 'e\u0301', 'é', '__proto__', 'constructor'];
  const value = linkedWire({ ids, parents: [], groups: [{ id: 'exact', members: ids }] });
  const context = validateContext(value); const state = apply(createState(context), context, 'Select', { reference: nodeReference(context, 'é') });
  assert.deepEqual(linkedIds(state), [longId, 'e\u0301', '__proto__', 'constructor']);
  invalidLinked((v) => { v.link_groups[0].members[0] = '😀'.repeat(1025); }, /string/);
});

test('v2 byte preflight rejects escaped-wire overflow before normalization and qualified-reference expansion', () => {
  const largeWire = linkedWire({ ids: Array.from({ length: 180 }, (_, i) => `n${i}`), parents: [], groups: [{ id: 'pair', members: ['n0', 'n1'] }] });
  for (const node of largeWire.nodes) node.origin.explanation = '\u0001'.repeat(4096);
  assert.ok(new TextEncoder().encode(JSON.stringify(largeWire)).length > 4 * 1024 * 1024);
  assert.throws(() => validateContext(largeWire), /byte limit/);
  const expanded = linkedWire({ ids: Array.from({ length: 1024 }, (_, i) => `n${i}`), parents: [], groups: [{ id: 'pair', members: ['n0', 'n1'] }] });
  expanded.document_id = 'd'.repeat(4096);
  assert.ok(new TextEncoder().encode(JSON.stringify(expanded)).length < 4 * 1024 * 1024);
  assert.throws(() => validateContext(expanded), /derived references.*byte limit/);
});

test('v2 normalized metadata also fits the byte budget even when wire fits exactly', () => {
  const value = linkedWire({ ids: Array.from({ length: 1024 }, (_, i) => `n${i}`), parents: [], groups: [{ id: 'pair', members: ['n0', 'n1'] }] });
  const budget = 4 * 1024 * 1024;
  let needed = budget - Buffer.byteLength(JSON.stringify(value));
  for (const node of value.nodes) {
    const size = Math.min(4096, needed); node.origin.explanation = 'x'.repeat(size); needed -= size;
  }
  assert.equal(needed, 0); assert.equal(Buffer.byteLength(JSON.stringify(value)), budget);
  assert.throws(() => validateContext(value), /normalized.*byte limit/);
  let removed = value.nodes.length * 18;
  for (const node of value.nodes) { const n = Math.min(removed, node.origin.explanation.length); node.origin.explanation = node.origin.explanation.slice(n); removed -= n; }
  assert.equal(removed, 0);
  const context = validateContext(value);
  assert.ok(Buffer.byteLength(JSON.stringify(context)) <= budget);
});

test('v2 initial state adds only group ID and immutable linked references', () => {
  const context = linkedFixture(); const state = createState(context);
  assert.deepEqual(Object.keys(state).sort(), [...Object.keys(createState(fixture())), 'selected_group_id', 'linked'].sort());
  assert.equal(state.selected_group_id, null); assert.deepEqual(state.linked, []); assert.ok(Object.isFrozen(state.linked));
  assert.equal(Object.hasOwn(createState(fixture()), 'linked'), false);
  assert.equal(Object.hasOwn(createState(fixture()), 'selected_group_id'), false);
});

test('v2 picker and click derive ordered links, repeat idempotently and rotate the primary', () => {
  const context = validateContext(linkedWire({ groups: [{ id: 'triple', members: ['root', 'child', 'other'] }] }));
  for (const type of ['Select', 'Click']) {
    let state = createState(context);
    const root = nodeReference(context, 'root'); const child = nodeReference(context, 'child');
    state = apply(state, context, type, { reference: child });
    assert.equal(state.selected, child); assert.equal(state.inspected, child); assert.equal(state.selected_group_id, 'triple');
    assert.deepEqual(linkedIds(state), ['root', 'other']); assert.ok(Object.isFrozen(state.linked));
    for (const ref of state.linked) { assert.equal(ref, nodeReference(context, ref.scene_node_id)); assert.ok(Object.isFrozen(ref)); }
    assert.equal(apply(state, context, type, { reference: { ...child } }), state);
    state = apply(state, context, type, { reference: root }); assert.equal(state.selected, root);
    assert.deepEqual(linkedIds(state), ['child', 'other']); assert.equal(state.selected_group_id, 'triple');
  }
});

test('v2 an ungrouped primary and clear remove all links without broadening by matching data key', () => {
  const value = linkedWire(); for (const node of value.nodes) node.origin.data_key = 'same';
  const context = validateContext(value); let state = createState(context);
  state = apply(state, context, 'Select', { reference: nodeReference(context, 'root') }); assert.deepEqual(linkedIds(state), ['child']);
  state = apply(state, context, 'Select', { reference: nodeReference(context, 'other') });
  assert.equal(state.selected.scene_node_id, 'other'); assert.equal(state.selected_group_id, null); assert.deepEqual(state.linked, []);
  state = apply(state, context, 'Select', { reference: nodeReference(context, 'child') });
  state = apply(state, context, 'Select', { reference: null });
  assert.equal(state.selected, null); assert.equal(state.selected_group_id, null); assert.deepEqual(state.linked, []);
  assert.equal(apply(state, context, 'Select', { reference: null }), state);
});

test('v2 exact ancestor/descendant members do not add siblings or descendants implicitly', () => {
  const value = linkedWire({ ids: ['root', 'parent', 'leaf', 'sibling', 'other'], parents: [null, 'root', 'parent', 'root', null], groups: [{ id: 'nested', members: ['root', 'leaf'] }] });
  for (const node of value.nodes) { node.origin.hir_node = 'same'; node.origin.data_key = 'same'; }
  const context = validateContext(value);
  let state = apply(createState(context), context, 'Select', { reference: nodeReference(context, 'leaf') });
  assert.deepEqual(linkedIds(state), ['root']); assert.equal(state.selected.scene_node_id, 'leaf');
  state = apply(state, context, 'Select', { reference: nodeReference(context, 'root') }); assert.deepEqual(linkedIds(state), ['leaf']);
  for (const id of ['parent', 'sibling', 'other']) {
    state = apply(state, context, 'Select', { reference: nodeReference(context, id) });
    assert.deepEqual(state.linked, []); assert.equal(state.selected_group_id, null);
  }
});

test('v2 hover/focus inspect exact latest origins without propagating selection', () => {
  const context = linkedFixture(); const root = nodeReference(context, 'root'); const child = nodeReference(context, 'child'); const other = nodeReference(context, 'other');
  let state = apply(createState(context), context, 'Select', { reference: root }); const links = state.linked;
  state = apply(state, context, 'Hover', { reference: child }); assert.equal(state.inspected, child);
  state = apply(state, context, 'Focus', { reference: other }); assert.equal(state.inspected, other);
  state = apply(state, context, 'Hover', { reference: child }); assert.equal(state.inspected, child);
  assert.equal(state.selected, root); assert.equal(state.linked, links); assert.equal(state.selected_group_id, 'pair');
  state = apply(state, context, 'Select', { reference: root }); assert.equal(state.inspected, root);
  assert.equal(apply(state, context, 'Select', { reference: root }), state);
  state = apply(state, context, 'Escape');
  assert.equal(state.selected, null); assert.equal(state.selected_group_id, null); assert.deepEqual(state.linked, []); assert.equal(state.inspected, other);
});

test('v2 reset preserves primary and links and Escape cancels drag before clearing them', () => {
  const context = linkedFixture(); const root = nodeReference(context, 'root');
  const selected = apply(createState(context), context, 'Select', { reference: root });
  const dragged = move(startDrag(context, selected), context);
  const reset = apply(dragged, context, 'Reset'); assert.deepEqual(reset.camera, { ...context.home, zoom: 1 });
  assert.equal(reset.selected, root); assert.equal(reset.linked, selected.linked); assert.equal(reset.selected_group_id, 'pair');
  const cancelled = apply(dragged, context, 'Escape'); assert.equal(cancelled.selected, root); assert.equal(cancelled.linked, selected.linked); assert.equal(cancelled.gesture, null);
  const cleared = apply(cancelled, context, 'Escape'); assert.equal(cleared.selected, null); assert.deepEqual(cleared.linked, []); assert.equal(cleared.selected_group_id, null);
});

test('v2 cancellation, late events and suppressed drag clicks preserve the whole selection', () => {
  const context = linkedFixture(); const root = nodeReference(context, 'root'); const child = nodeReference(context, 'child');
  const selected = apply(createState(context), context, 'Select', { reference: root });
  const dragged = move(startDrag(context, selected), context);
  for (const action of [{ type: 'CancelGesture' }, { type: 'TogglePan' }, { type: 'PointerStart', pointer_id: 8 }, { type: 'Pan', dx: 1, dy: 0 }, { type: 'ZoomAt', factor: 1.25 }]) {
    const cancelled = reduce(dragged, action, context);
    assert.equal(cancelled.gesture, null); assert.equal(cancelled.selected, root); assert.equal(cancelled.linked, selected.linked); assert.equal(cancelled.selected_group_id, 'pair');
    assert.equal(move(cancelled, context), cancelled);
    const suppressed = apply(cancelled, context, 'Click', { reference: child }); assert.equal(suppressed.selected, root); assert.equal(suppressed.linked, selected.linked);
    const nextClick = apply(suppressed, context, 'Click', { reference: child }); assert.equal(nextClick.selected, child); assert.deepEqual(linkedIds(nextClick), ['root']);
  }
});

test('v2 canonical groups bind retained state even when document, scene and instance match', () => {
  const source = linkedWire(); const context = validateContext(source); const state = createState(context);
  assert.equal(apply(state, validateContext(structuredClone(source)), 'Select', { reference: null }), state);
  for (const groups of [[{ id: 'different', members: ['root', 'child'] }], [{ id: 'pair', members: ['root', 'other'] }]]) {
    const changed = structuredClone(source); changed.link_groups = groups;
    assert.throws(() => apply(state, validateContext(changed), 'Reset'), /state belongs/);
  }
  assert.throws(() => apply(state, fixture(), 'Reset'), /state belongs/);
  assert.throws(() => apply(createState(fixture()), context, 'Reset'), /state belongs/);
});

test('mixed v1/v2 contexts isolate instance-qualified references and retain shared revision checks', () => {
  const v1 = fixture(); const v2 = validateContext(linkedWire({ key: 'linked' }));
  const one = apply(createState(v1), v1, 'Select', { reference: nodeReference(v1, 'child') });
  const two = apply(createState(v2), v2, 'Click', { reference: nodeReference(v2, 'root') });
  assert.equal(Object.hasOwn(one, 'linked'), false); assert.deepEqual(linkedIds(two), ['child']);
  assert.equal(apply(one, v1, 'Select', { reference: two.selected }), one);
  assert.equal(apply(two, v2, 'Select', { reference: one.selected }), two);
  assert.throws(() => apply(one, v2, 'Reset'), /state belongs/);
  assert.throws(() => apply(two, v1, 'Reset'), /state belongs/);
  assert.equal(validateRuntimeRevision(v1.runtime_payload_sha256, v2.runtime_payload_sha256), v1.runtime_payload_sha256);
  const changed = linkedWire({ key: 'linked' }); changed.runtime_payload_sha256 = 'f'.repeat(64);
  assert.throws(() => validateRuntimeRevision(v1.runtime_payload_sha256, changed.runtime_payload_sha256), /incompatible/);
  assert.throws(() => apply(two, validateContext(changed), 'Reset'), /state belongs/);
});

test('linked DOM projection structural audit uses exact nodes, independent text and existing cleanup only', async () => {
  const source = await readFile(new URL('./runtime.mjs', import.meta.url), 'utf8');
  assert.match(source, /for \(const ref of renderedLinked \?\? \[\]\) elements\.get\(ref\.scene_node_id\)\.classList\.remove\('vizir-is-linked'\)/);
  assert.match(source, /for \(const ref of state\.linked\) \{\s+const element = elements\.get\(ref\.scene_node_id\);\s+element\.classList\.add\('vizir-is-linked'\);\s+element\.setAttribute\('tabindex', '-1'\)/);
  assert.match(source, /Link group: \$\{state\.selected_group_id \?\? 'none'\}\. Linked nodes: \$\{state\.linked\.length\}/);
  assert.match(source, /linked_count: state\.linked\.length/);
  assert.match(source, /origin: contextData\(context\)\.index\.get\(inspect\.scene_node_id\)\.origin, \.\.\.selectionInfo\(state\)/);
  assert.match(source, /\['class', 'tabindex', 'aria-label'\]/);
  assert.match(source, /for \(const restore of restores\.reverse\(\)\) restore\(\)/);
  assert.doesNotMatch(source, /querySelector(?:All)?\([^\n]*vizir-is-linked/);
  assert.match(source, /version: FORMAT, profile: PROFILE, supported_profiles: SUPPORTED_PROFILES/);
  // These are source/contract checks. They do not run DOM, focus or cleanup APIs.
});

test('v2 derived linked-state envelope is bounded before selecting any primary', () => {
  const ids = Array.from({ length: 256 }, (_, i) => `n${i}`);
  const value = linkedWire({ ids, parents: [], groups: [{ id: 'all', members: ids }] });
  value.document_id = '\u0001'.repeat(2680);
  const binding = { document_id: value.document_id, scene_sha256: value.scene_sha256, instance_key: value.instance_key };
  const bindingBytes = Buffer.byteLength(JSON.stringify(binding));
  const refBytes = ids.map((id) => bindingBytes + 17 + Buffer.byteLength(JSON.stringify(id)));
  assert.ok(refBytes.reduce((a, b) => a + b, 0) < 4 * 1024 * 1024);
  assert.ok(bindingBytes + 3 * Math.max(...refBytes) + 2048 + refBytes.reduce((a, b) => a + b + 1, 0) > 4 * 1024 * 1024);
  assert.throws(() => validateContext(value), /derived linked state.*byte limit/);
  value.document_id = '\u0001'.repeat(2600);
  const context = validateContext(value); let state = createState(context);
  state = apply(state, context, 'Select', { reference: nodeReference(context, ids[0]) });
  state = apply(state, context, 'Hover', { reference: nodeReference(context, ids[1]) });
  state = apply(state, context, 'Focus', { reference: nodeReference(context, ids[2]) });
  state = move(startDrag(context, state), context);
  assert.equal(state.linked.length, 255); assert.ok(Buffer.byteLength(JSON.stringify(state)) < 4 * 1024 * 1024);
});

// V1 oracle begin. Fixed bounded trace generated against the published frozen-v3
// payload SHA-256 49b480dfe490e8e3e5d903989f3d58c9d243db8d8e1fc0bf15cb471f99a7bb36
// from baseline 480772e4d9760579e7b7b900fa832701e2cf0d30. State has no
// payload-revision field, so the only permitted payload-identity change is inert.
const V1_GOLDEN_ACTIONS = [
  { type: 'Select', id: 'child' }, { type: 'Select', id: 'child' },
  { type: 'Hover', id: 'root' }, { type: 'Focus', id: 'other' },
  { type: 'Select', id: 'child' }, { type: 'Click', id: 'child' },
  { type: 'ZoomAt', factor: 2, x: 200, y: 100 }, { type: 'Pan', dx: 1, dy: -1 },
  { type: 'TogglePan' },
  { type: 'PointerStart', pointer_id: 7, css_x: 100, css_y: 100, scene_x: 500, scene_y: 250 },
  { type: 'PointerMove', pointer_id: 7, css_x: 102, css_y: 102, scene_x: 510, scene_y: 260 },
  { type: 'PointerMove', pointer_id: 7, css_x: 105, css_y: 100, scene_x: 508, scene_y: 250 },
  { type: 'PointerEnd', pointer_id: 8 }, { type: 'PointerEnd', pointer_id: 7 },
  { type: 'Click', id: 'other' }, { type: 'Click', id: 'other' },
  { type: 'Hover', id: null }, { type: 'Focus', id: null }, { type: 'Escape' },
  { type: 'Select', id: 'root' },
  { type: 'PointerStart', pointer_id: 9, css_x: 20, css_y: 30, scene_x: 200, scene_y: 100 },
  { type: 'PointerMove', pointer_id: 9, css_x: 40, css_y: 50, scene_x: 300, scene_y: 200 },
  { type: 'Escape' }, { type: 'Escape' }, { type: 'PrepareClick' },
  { type: 'Click', id: 'child' }, { type: 'Reset' },
  { type: 'ZoomAt', factor: Number.MAX_VALUE }, { type: 'Pan', dx: Number.MAX_VALUE, dy: -Number.MAX_VALUE },
  { type: 'ZoomAt', factor: Number.MIN_VALUE }, { type: 'ZoomAt', factor: NaN },
  { type: 'Select', id: 'other', foreign: true }, { type: 'CancelGesture' },
  { type: 'TogglePan' }, { type: 'Select', id: null }, { type: 'Select', id: null },
];
function v1TraceSnapshots(runtime) {
  const context = runtime.validateContext(wire());
  let state = runtime.createState(context);
  const result = [{ same: false, state }];
  for (const entry of V1_GOLDEN_ACTIONS) {
    const { id, foreign, ...action } = entry;
    if (Object.hasOwn(entry, 'id')) action.reference = id === null ? null : runtime.nodeReference(context, id);
    if (foreign) action.reference = { ...action.reference, instance_key: 'foreign' };
    const previous = state; state = runtime.reduce(state, action, context);
    result.push({ same: state === previous, state });
  }
  return result;
}
// V1 oracle end.

test('v1 reducer full state and identity trace matches its fixed published-runtime golden', () => {
  const trace = v1TraceSnapshots({ validateContext, createState, reduce, nodeReference });
  const digest = createHash('sha256').update(JSON.stringify(trace)).digest('hex');
  assert.equal(trace.length, 37);
  assert.equal(digest, '896bbcda4e6f777b89ed8d5c78af1c47e8798a219a6c8ecbdfafec4c5333e2ee');
  for (const { state } of trace) {
    assert.equal(Object.hasOwn(state, 'linked'), false); assert.equal(Object.hasOwn(state, 'selected_group_id'), false);
  }
});

test('v2 preserves Origin null normalization and exact mixed Unicode JSON budget accounting', () => {
  const value = linkedWire();
  value.document_id = '"\\\b\t\n\f\r\u0001/é\u2028😀';
  value.nodes[0].origin.data_key = null;
  value.nodes[1].origin.data_lineage = ['"\\\u0000', 'é', '😀', '\u2028'];
  const omitted = structuredClone(value); delete omitted.nodes[0].origin.data_key;
  const context = validateContext(value);
  assert.deepEqual(context, validateContext(omitted));
  const state = apply(createState(context), context, 'Select', { reference: nodeReference(context, 'root') });
  assert.equal(state.linked[0].document_id, value.document_id);
  assert.equal(state.linked[0].scene_node_id, 'child');
  assert.equal(new Map(expectedOriginAttributes(context.nodes[0].origin)).get('data-key'), null);
  assert.equal(apply(state, validateContext(omitted), 'Select', { reference: nodeReference(context, 'root') }), state);
});

test('v2 two ungrouped equal-data-key nodes never join and background click clears primary plus links', () => {
  const value = linkedWire({ ids: ['root', 'child', 'other', 'equal'], parents: [null, 'root', null, null] });
  value.nodes[2].origin.data_key = 'same'; value.nodes[3].origin.data_key = 'same';
  const context = validateContext(value); let state = createState(context);
  for (const id of ['other', 'equal']) {
    state = apply(state, context, 'Click', { reference: nodeReference(context, id) });
    assert.equal(state.selected.scene_node_id, id); assert.equal(state.selected_group_id, null); assert.deepEqual(state.linked, []);
  }
  state = apply(state, context, 'Select', { reference: nodeReference(context, 'child') });
  assert.deepEqual(linkedIds(state), ['root']);
  state = apply(state, context, 'Click', { reference: null });
  assert.equal(state.selected, null); assert.equal(state.selected_group_id, null); assert.deepEqual(state.linked, []);
  assert.equal(apply(state, context, 'Click', { reference: null }), state);
});

test('v2 compact link-group metadata accepts exactly 1 MiB and rejects one byte more', () => {
  const ids = Array.from({ length: 200 }, (_, i) => '\t'.repeat(2500) + String(i).padStart(3, '0'));
  const groups = [{ id: 'group', members: ids }];
  const groupBudget = 1024 * 1024;
  let needed = groupBudget - Buffer.byteLength(JSON.stringify(groups));
  for (let i = 0; i < ids.length && needed; i += 1) {
    const room = 4096 - Buffer.byteLength(ids[i]);
    const tabs = Math.min(Math.floor(needed / 2), room);
    ids[i] = '\t'.repeat(tabs) + ids[i]; needed -= tabs * 2;
    if (needed === 1 && tabs < room) { ids[i] += 'x'; needed -= 1; }
  }
  assert.equal(needed, 0); assert.equal(Buffer.byteLength(JSON.stringify(groups)), groupBudget);
  const value = linkedWire({ ids, parents: [], groups });
  assert.ok(Buffer.byteLength(JSON.stringify(value)) < 4 * 1024 * 1024);
  assert.equal(validateContext(value).link_groups[0].members.length, 200);
  const last = value.nodes.at(-1); last.scene_node_id += 'x'; last.dom_id = nodeDomId(value.instance_key, last.scene_node_id);
  value.link_groups[0].members[ids.length - 1] = last.scene_node_id;
  assert.ok(Buffer.byteLength(last.scene_node_id) <= 4096);
  assert.equal(Buffer.byteLength(JSON.stringify(value.link_groups)), groupBudget + 1);
  assert.ok(Buffer.byteLength(JSON.stringify(value)) < 4 * 1024 * 1024);
  assert.throws(() => validateContext(value), /byte limit/);
});

test('pure selection-info projection lists exact linked IDs in order without changing Origin or v1 output', () => {
  const context = validateContext(linkedWire({ groups: [{ id: 'triple', members: ['root', 'child', 'other'] }] }));
  let state = apply(createState(context), context, 'Select', { reference: nodeReference(context, 'child') });
  const before = JSON.stringify(state); const origins = JSON.stringify(context.nodes.map((node) => node.origin));
  assert.deepEqual(selectionInfo(state), { selected_group_id: 'triple', linked_count: 2, linked_scene_node_ids: ['root', 'other'] });
  assert.equal(JSON.stringify(state), before);
  state = apply(state, context, 'Hover', { reference: nodeReference(context, 'other') });
  assert.equal(state.inspected.scene_node_id, 'other');
  assert.deepEqual(selectionInfo(state), { selected_group_id: 'triple', linked_count: 2, linked_scene_node_ids: ['root', 'other'] });
  assert.equal(JSON.stringify(context.nodes.map((node) => node.origin)), origins);
  state = apply(state, context, 'Select', { reference: nodeReference(context, 'root') });
  assert.deepEqual(selectionInfo(state), { selected_group_id: 'triple', linked_count: 2, linked_scene_node_ids: ['child', 'other'] });
  state = apply(state, context, 'Escape');
  assert.deepEqual(selectionInfo(state), { selected_group_id: null, linked_count: 0, linked_scene_node_ids: [] });
  const v1 = fixture(); const legacy = apply(createState(v1), v1, 'Select', { reference: nodeReference(v1, 'child') });
  assert.deepEqual(selectionInfo(createState(v1)), {}); assert.deepEqual(selectionInfo(legacy), {});
  assert.throws(() => selectionInfo(structuredClone(state)), /state belongs/);
});

test('selection-info projection preserves all 255 linked IDs without truncation or reinterpretation', () => {
  const ids = Array.from({ length: 256 }, (_, i) => `n${i}<>&"\t`);
  const context = validateContext(linkedWire({ ids, parents: [], groups: [{ id: 'all', members: ids }] }));
  const primary = ids[128];
  const state = apply(createState(context), context, 'Select', { reference: nodeReference(context, primary) });
  const projected = selectionInfo(state);
  assert.equal(projected.linked_count, 255);
  assert.deepEqual(projected.linked_scene_node_ids, ids.filter((id) => id !== primary));
  assert.equal(projected.linked_scene_node_ids.includes(primary), false);
  assert.equal(projected.linked_scene_node_ids.at(-1), ids.at(-1));
  assert.deepEqual(JSON.parse(JSON.stringify(projected)), projected);
  projected.linked_scene_node_ids.length = 0;
  assert.equal(state.linked.length, 255);
});
