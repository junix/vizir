/** Native tests of metadata and the pure reducer only.
 * These are not DOM, browser, accessibility, CSP enforcement or pointer-capture
 * acceptance tests. Browser acceptance is explicitly BLOCKED / NOT RUN.
 */
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { validateContext, createState, reduce, nodeDomId, nodeReference, viewportKeyAction, validateRuntimeRevision, expectedOriginAttributes, bootstrap } from './runtime.mjs';

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
