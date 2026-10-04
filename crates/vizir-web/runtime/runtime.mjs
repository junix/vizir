/** VizIR explorer-v1. No dependencies, network, timers, source mutation or animation.
 * The digest binds references to the emitted snapshot; it is not authentication.
 * Only compiler-generated fragments are supported. Call dispose before replacing a
 * fragment; callers must not mutate a generated subtree while it is mounted.
 */
const FORMAT = 'vizir-interaction/1';
const PROFILE = 'explorer-v1';
// The exporter prefixes this immutable declaration before these exact payload bytes.
const PAYLOAD_SHA256 = typeof VIZIR_RUNTIME_PAYLOAD_SHA256 === 'string' ? VIZIR_RUNTIME_PAYLOAD_SHA256 : null;
const MAX_BYTES = 4 * 1024 * 1024;
const SVG_NS = 'http://www.w3.org/2000/svg';
const contexts = new WeakMap();
const retainedStates = new WeakMap();
const utf8 = new TextEncoder();
const REGISTRY = Symbol.for('vizir.explorer-v1.registry');
const FIXED_CAMERA = Object.freeze({ min_zoom: 1, max_zoom: 16, zoom_factor: 1.25, pan_fraction: 0.1, drag_threshold_css_px: 4 });
const own = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
const fail = (message) => { throw new TypeError(`VizIR explorer: ${message}`); };
const finite = (value) => typeof value === 'number' && Number.isFinite(value);
const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

function record(value, required, optional = []) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail('expected a metadata object');
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) fail('metadata must contain plain objects');
  const keys = Reflect.ownKeys(value);
  if (keys.some((key) => typeof key !== 'string' || !required.includes(key) && !optional.includes(key)) || required.some((key) => !own(value, key))) fail('unknown or missing metadata field');
  for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(value))) {
    if (!own(descriptor, 'value') || !descriptor.enumerable) fail('metadata accessors and hidden fields are unsupported');
  }
}
function array(value, max = MAX_BYTES) {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype || value.length > max || Reflect.ownKeys(value).length !== value.length + 1) fail('invalid metadata array');
  for (let i = 0; i < value.length; i += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(i));
    if (!descriptor || !own(descriptor, 'value') || !descriptor.enumerable) fail('invalid metadata array entry');
  }
}
function string(value, nonblank = false) {
  if (typeof value !== 'string' || value.length > 4096 || utf8.encode(value).length > 4096 || nonblank && /^\p{White_Space}*$/u.test(value)) fail('invalid metadata string');
  // UTF-8 replaces lone surrogates. Reject them to keep DOM ID encoding injective.
  for (const character of value) {
    const cp = character.codePointAt(0);
    if (cp >= 0xd800 && cp <= 0xdfff) fail('metadata contains an unpaired surrogate');
  }
  return value;
}
function deepFreeze(value) {
  if (value && typeof value === 'object' && !Object.isFrozen(value)) {
    for (const child of Object.values(value)) deepFreeze(child);
    Object.freeze(value);
  }
  return value;
}
export function nodeDomId(instanceKey, sceneNodeId) {
  return `vzi-${instanceKey}-node-${Array.from(utf8.encode(sceneNodeId), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
}

/** Validate exact wire fields, bounds and preorder; return an independent frozen copy. */
export function validateContext(input) {
  if (contexts.has(input)) return input;
  record(input, ['format', 'profile', 'document_id', 'scene_sha256', 'runtime_payload_sha256', 'instance_key', 'home', 'camera', 'nodes']);
  if (input.format !== FORMAT || input.profile !== PROFILE) fail('unsupported interaction format or profile');
  let sourceBytes = 0;
  const sourceString = (value, nonblank = false) => {
    const text = string(value, nonblank);
    sourceBytes += utf8.encode(text).length;
    if (sourceBytes > MAX_BYTES) fail('aggregate source strings exceed byte limit');
    return text;
  };
  const documentId = sourceString(input.document_id, true);
  if (typeof input.scene_sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(input.scene_sha256)) fail('invalid scene digest');
  validateRuntimeRevision(input.runtime_payload_sha256, input.runtime_payload_sha256);
  if (typeof input.instance_key !== 'string' || !/^[a-z][a-z0-9-]{0,31}$/.test(input.instance_key)) fail('invalid instance key');
  record(input.home, ['x', 'y', 'width', 'height']);
  if (input.home.x !== 0 || input.home.y !== 0 || !finite(input.home.width) || !finite(input.home.height) || input.home.width < 1 || input.home.height < 1 || input.home.width > 1e6 || input.home.height > 1e6) fail('invalid home rectangle');
  record(input.camera, Object.keys(FIXED_CAMERA));
  if (Object.entries(FIXED_CAMERA).some(([key, value]) => input.camera[key] !== value)) fail('unsupported camera policy');
  array(input.nodes, 8192);
  const nodes = [];
  const index = new Map();
  const activeParents = [];
  let lineageCount = 0;
  for (const node of input.nodes) {
    record(node, ['scene_node_id', 'parent_scene_node_id', 'dom_id', 'origin']);
    const id = sourceString(node.scene_node_id, true);
    if (index.has(id)) fail('duplicate scene node identity');
    if (node.dom_id !== nodeDomId(input.instance_key, id)) fail('incorrect namespaced DOM identity');
    if (node.parent_scene_node_id !== null) string(node.parent_scene_node_id, true);
    const parent = node.parent_scene_node_id;
    if (parent === null) activeParents.length = 0;
    else {
      const position = activeParents.indexOf(parent);
      if (position < 0) fail('parent is absent, later, or outside preorder ancestry');
      activeParents.length = position + 1;
    }
    activeParents.push(id);
    if (activeParents.length > 64) fail('scene tree exceeds depth limit');
    record(node.origin, ['hir_node', 'mir_node', 'generated_by', 'explanation'], ['data_key', 'data_lineage']);
    const origin = {
      hir_node: sourceString(node.origin.hir_node, true),
      mir_node: sourceString(node.origin.mir_node, true),
      generated_by: sourceString(node.origin.generated_by, true),
      explanation: sourceString(node.origin.explanation),
    };
    if (own(node.origin, 'data_key') && node.origin.data_key !== null) origin.data_key = sourceString(node.origin.data_key);
    if (own(node.origin, 'data_lineage')) {
      array(node.origin.data_lineage, 32768);
      lineageCount += node.origin.data_lineage.length;
      if (lineageCount > 32768) fail('lineage entry count exceeds limit');
      origin.data_lineage = node.origin.data_lineage.map((entry) => sourceString(entry));
    } else origin.data_lineage = [];
    const normalized = { scene_node_id: id, parent_scene_node_id: parent, dom_id: node.dom_id, origin };
    nodes.push(normalized);
    index.set(id, normalized);
  }
  const result = { format: FORMAT, profile: PROFILE, document_id: documentId, scene_sha256: input.scene_sha256, runtime_payload_sha256: input.runtime_payload_sha256, instance_key: input.instance_key, home: { x: 0, y: 0, width: input.home.width, height: input.home.height }, camera: { ...FIXED_CAMERA }, nodes };
  if (utf8.encode(JSON.stringify(input)).length > MAX_BYTES) fail('interaction metadata exceeds byte limit');
  deepFreeze(result);
  const binding = deepFreeze({ document_id: result.document_id, scene_sha256: result.scene_sha256, instance_key: result.instance_key });
  const references = new Map(nodes.map((node) => [node.scene_node_id, deepFreeze({ ...binding, scene_node_id: node.scene_node_id })]));
  contexts.set(result, { index, binding, references, signature: JSON.stringify(result) });
  return result;
}
function contextData(context) { return contexts.get(validateContext(context)); }
export function nodeReference(context, id) { return contextData(context).references.get(id) ?? null; }
function reference(context, value) {
  if (value === null) return null;
  if (!value || typeof value !== 'object') return undefined;
  const data = contextData(context);
  if (value.document_id !== data.binding.document_id || value.scene_sha256 !== data.binding.scene_sha256 || value.instance_key !== data.binding.instance_key) return undefined;
  return data.references.get(value.scene_node_id);
}
function homeCamera(context) { return { ...context.home, zoom: 1 }; }
function retain(state, context) { deepFreeze(state); retainedStates.set(state, context); return state; }
function nextState(state, patch) { return retain({ ...state, ...patch }, retainedStates.get(state)); }
export function createState(rawContext) {
  const context = validateContext(rawContext);
  return retain({ binding: contextData(context).binding, camera: homeCamera(context), selected: null, hovered: null, focused: null, inspected: null, pan_mode: false, gesture: null, suppress_click: false }, context);
}
function cameraAt(context, zoom, x, y) {
  const width = context.home.width / zoom;
  const height = context.home.height / zoom;
  return { x: clamp(x, 0, context.home.width - width), y: clamp(y, 0, context.home.height - height), width, height, zoom };
}
function cancelled(state) { return nextState(state, { gesture: null, suppress_click: state.suppress_click || state.gesture !== null }); }

/** Pure state machine. Actions use normalized coordinates and snapshot references. */
export function reduce(state, action, rawContext) {
  const context = validateContext(rawContext);
  const binding = contextData(context).binding;
  const originalContext = retainedStates.get(state);
  if (!originalContext || Object.keys(binding).some((key) => state.binding?.[key] !== binding[key]) || originalContext !== context && contextData(originalContext).signature !== contextData(context).signature) fail('state belongs to another instance or snapshot');
  if (!action || typeof action.type !== 'string') fail('invalid action');
  switch (action.type) {
    case 'ZoomAt': {
      if (!finite(action.factor) || action.factor <= 0) return state;
      const old = state.camera;
      const x = action.x ?? old.x + old.width / 2;
      const y = action.y ?? old.y + old.height / 2;
      if (!finite(x) || !finite(y)) return state;
      const zoom = clamp(old.zoom * action.factor, 1, 16);
      if (zoom === old.zoom) return state;
      const ratio = old.zoom / zoom;
      const anchorX = clamp(x, old.x, old.x + old.width);
      const anchorY = clamp(y, old.y, old.y + old.height);
      return nextState(cancelled(state), { camera: cameraAt(context, zoom, anchorX - (anchorX - old.x) * ratio, anchorY - (anchorY - old.y) * ratio) });
    }
    case 'Pan': {
      if (!finite(action.dx) || !finite(action.dy)) return state;
      const old = state.camera;
      // Bound huge external inputs before arithmetic; all retained camera math is finite.
      const x = old.x + clamp(action.dx, -1, 1) * old.width * context.camera.pan_fraction;
      const y = old.y + clamp(action.dy, -1, 1) * old.height * context.camera.pan_fraction;
      return nextState(cancelled(state), { camera: cameraAt(context, old.zoom, x, y) });
    }
    case 'Reset': return nextState(cancelled(state), { camera: homeCamera(context) });
    case 'Select': {
      const selected = reference(context, action.reference);
      if (selected === undefined || selected?.scene_node_id === state.selected?.scene_node_id && selected?.scene_node_id === state.inspected?.scene_node_id) return state;
      return nextState(state, { selected, inspected: selected });
    }
    case 'Hover':
    case 'Focus': {
      const ref = reference(context, action.reference);
      const key = action.type === 'Hover' ? 'hovered' : 'focused';
      if (ref === undefined) return state;
      // New focus/picker intent wins over stale pointer location; new hover still works.
      const inspected = ref ?? (key === 'hovered' ? state.focused : state.hovered) ?? state.selected;
      if (ref?.scene_node_id === state[key]?.scene_node_id && inspected?.scene_node_id === state.inspected?.scene_node_id) return state;
      return nextState(state, { [key]: ref, inspected });
    }
    case 'TogglePan': return nextState(cancelled(state), { pan_mode: !state.pan_mode });
    case 'PointerStart': {
      if (state.gesture) return cancelled(state);
      if (!state.pan_mode || !Number.isSafeInteger(action.pointer_id) || action.pointer_id < 0 || ![action.css_x, action.css_y, action.scene_x, action.scene_y].every(finite)) return state;
      return nextState(state, { suppress_click: false, gesture: { pointer_id: action.pointer_id, start_css_x: action.css_x, start_css_y: action.css_y, start_scene_x: action.scene_x, start_scene_y: action.scene_y, camera: state.camera, dragging: false } });
    }
    case 'PointerMove': {
      const gesture = state.gesture;
      if (!gesture || action.pointer_id !== gesture.pointer_id || ![action.css_x, action.css_y, action.scene_x, action.scene_y].every(finite)) return state;
      const distance = Math.hypot(action.css_x - gesture.start_css_x, action.css_y - gesture.start_css_y);
      const dragging = gesture.dragging || distance >= context.camera.drag_threshold_css_px;
      if (!dragging) return state;
      const x = gesture.camera.x + (gesture.start_scene_x - action.scene_x);
      const y = gesture.camera.y + (gesture.start_scene_y - action.scene_y);
      if (!finite(x) || !finite(y)) return state;
      return nextState(state, { camera: cameraAt(context, gesture.camera.zoom, x, y), gesture: { ...gesture, dragging: true } });
    }
    case 'PointerEnd': {
      if (!state.gesture || action.pointer_id !== state.gesture.pointer_id) return state;
      return nextState(state, { gesture: null, suppress_click: state.gesture.dragging });
    }
    case 'CancelGesture': return state.gesture ? cancelled(state) : state;
    case 'PrepareClick': return state.suppress_click ? nextState(state, { suppress_click: false }) : state;
    case 'Click': {
      if (state.suppress_click) return nextState(state, { suppress_click: false });
      const selected = reference(context, action.reference);
      return selected === undefined || selected?.scene_node_id === state.selected?.scene_node_id && selected?.scene_node_id === state.inspected?.scene_node_id ? state : nextState(state, { selected, inspected: selected });
    }
    case 'Escape': return state.gesture ? cancelled(state) : state.selected ? nextState(state, { selected: null, inspected: state.focused ?? state.hovered }) : state;
    default: fail('unsupported action');
  }
}

/** Normalize only viewport keys; the DOM adapter separately checks exact focus ownership. */
export function viewportKeyAction(event) {
  if (!event || event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey && event.key !== '+' || event.isComposing) return null;
  const keys = { ArrowLeft: { type: 'Pan', dx: -1, dy: 0 }, ArrowRight: { type: 'Pan', dx: 1, dy: 0 }, ArrowUp: { type: 'Pan', dx: 0, dy: -1 }, ArrowDown: { type: 'Pan', dx: 0, dy: 1 }, '+': { type: 'ZoomAt', factor: 1.25 }, '=': { type: 'ZoomAt', factor: 1.25 }, '-': { type: 'ZoomAt', factor: 1 / 1.25 }, Home: { type: 'Reset' }, '0': { type: 'Reset' }, Escape: { type: 'Escape' } };
  return own(keys, event.key) ? keys[event.key] : null;
}

/** Exact payload compatibility, not a claim of publisher authenticity. */
export function validateRuntimeRevision(expected, actual) {
  if (typeof expected !== 'string' || !/^[0-9a-f]{64}$/.test(expected) || typeof actual !== 'string' || !/^[0-9a-f]{64}$/.test(actual)) fail('missing or invalid runtime payload revision');
  if (expected !== actual) fail('incompatible runtime payload revision');
  return expected;
}
function registry(globalObject) {
  validateRuntimeRevision(PAYLOAD_SHA256, PAYLOAD_SHA256);
  if (own(globalObject, REGISTRY)) {
    const current = globalObject[REGISTRY];
    if (current?.version !== FORMAT) fail('incompatible runtime registry');
    validateRuntimeRevision(PAYLOAD_SHA256, current.runtime_payload_sha256);
    return current;
  }
  const value = { version: FORMAT, runtime_payload_sha256: PAYLOAD_SHA256, mounted: new WeakMap(), listening: new WeakSet(), api: null };
  Object.defineProperty(globalObject, REGISTRY, { value, enumerable: false, configurable: false, writable: false });
  return value;
}
/** Attribute expectations for a normalized Origin. Null means the attribute is absent. */
export function expectedOriginAttributes(origin) {
  return [['data-hir-node', origin.hir_node], ['data-mir-node', origin.mir_node], ['data-generated-by', origin.generated_by], ['data-key', origin.data_key ?? null], ['data-lineage', origin.data_lineage.length ? origin.data_lineage.join(',') : null]];
}
function generatedOwner(element) { return element?.closest?.('[data-vizir-explorer]') ?? null; }
function requireDOM(condition, message) { if (!condition) fail(message); }
function validateDOM(root) {
  requireDOM(root?.nodeType === 1 && root.localName === 'section' && root.namespaceURI === 'http://www.w3.org/1999/xhtml' && root.getAttribute('data-vizir-explorer') === '1', 'expected generated explorer section');
  const document = root.ownerDocument;
  requireDOM(document?.documentElement?.contains(root), 'fragment must be connected to its owner document');
  const find = (selector, localName) => {
    const elements = root.querySelectorAll(selector);
    requireDOM(elements.length === 1 && generatedOwner(elements[0]) === root && (!localName || elements[0].localName === localName), 'missing, duplicate, or foreign explorer control');
    return elements[0];
  };
  const metadata = find('[data-vizir-metadata]', 'script');
  requireDOM(metadata.getAttribute('type') === 'application/json' && !metadata.hasAttribute('src'), 'invalid metadata container');
  requireDOM(metadata.textContent.length <= MAX_BYTES && utf8.encode(metadata.textContent).length <= MAX_BYTES, 'interaction metadata exceeds byte limit');
  let parsed;
  try { parsed = JSON.parse(metadata.textContent); } catch { fail('metadata is not valid JSON'); }
  const context = validateContext(parsed);
  validateRuntimeRevision(PAYLOAD_SHA256, context.runtime_payload_sha256);
  requireDOM(root.getAttribute('data-vizir-instance') === context.instance_key, 'fragment instance mismatch');
  const roots = Array.from(document.querySelectorAll('[data-vizir-explorer]'));
  requireDOM(roots.filter((candidate) => candidate.getAttribute('data-vizir-instance') === context.instance_key).length === 1, 'duplicate instance key in document');
  requireDOM(root.querySelectorAll('[data-vizir-explorer]').length === 0 && generatedOwner(root.parentElement) === null, 'nested explorer fragments are unsupported');
  const viewport = find('[data-vizir-viewport]', 'div');
  const svg = find('[data-vizir-canvas]', 'svg');
  const picker = find('[data-vizir-picker]', 'select');
  const status = find('[data-vizir-status]', 'span');
  const inspector = find('[data-vizir-inspector]', 'pre');
  const toolbar = find('.vizir-toolbar', 'div');
  requireDOM(toolbar.parentElement === root && toolbar.getAttribute('role') === 'group' && Boolean(toolbar.getAttribute('aria-label')), 'invalid toolbar semantics');
  requireDOM(viewport.parentElement === root && inspector.parentElement === root && metadata.parentElement === root && picker.parentElement === toolbar && status.parentElement === toolbar, 'invalid control ownership');
  requireDOM(Boolean(inspector.getAttribute('aria-label')) && Boolean(viewport.getAttribute('aria-label')), 'missing explorer accessible name');
  requireDOM(svg.namespaceURI === SVG_NS && svg.parentElement === viewport && svg.getAttribute('data-vizir-canvas') === '1' && svg.getAttribute('role') === 'img', 'invalid generated SVG ownership');
  requireDOM(viewport.getAttribute('tabindex') === '0' && viewport.getAttribute('role') === 'group', 'invalid viewport semantics');
  requireDOM(picker.id === `vzi-${context.instance_key}-control-picker` && picker.options.length === 1 && picker.options[0].value === '', 'invalid source picker');
  const labels = toolbar.querySelectorAll('label');
  requireDOM(labels.length === 1 && labels[0].getAttribute('for') === picker.id && Boolean(labels[0].textContent), 'invalid source picker label');
  requireDOM(status.getAttribute('role') === 'status' && status.getAttribute('aria-live') === 'polite', 'invalid status semantics');
  const buttons = new Map();
  for (const button of root.querySelectorAll('[data-vizir-action]')) {
    const action = button.getAttribute('data-vizir-action');
    requireDOM(button.localName === 'button' && button.getAttribute('type') === 'button' && button.parentElement === toolbar && generatedOwner(button) === root && ['zoom-in', 'zoom-out', 'reset', 'pan'].includes(action) && !buttons.has(action), 'invalid explorer action');
    requireDOM(button.id === `vzi-${context.instance_key}-control-${action}`, 'incorrect control identity');
    buttons.set(action, button);
  }
  requireDOM(buttons.size === 4 && buttons.get('pan').getAttribute('aria-pressed') === 'false', 'missing explorer action or invalid pan state');
  requireDOM(['setPointerCapture', 'releasePointerCapture', 'hasPointerCapture'].every((name) => typeof viewport[name] === 'function') && typeof svg.getScreenCTM === 'function', 'required pointer or SVG coordinate API is unavailable');
  const home = context.home;
  const viewBox = (svg.getAttribute('viewBox') ?? '').trim().split(/[\s,]+/).map(Number);
  requireDOM(viewBox.length === 4 && viewBox.every(finite) && viewBox[0] === 0 && viewBox[1] === 0 && viewBox[2] === home.width && viewBox[3] === home.height, 'static SVG camera mismatch');
  requireDOM(Number(svg.getAttribute('width')) === home.width && Number(svg.getAttribute('height')) === home.height, 'static SVG dimensions mismatch');
  const titleId = `vzi-${context.instance_key}-title`;
  const markerId = `vzi-${context.instance_key}-marker`;
  requireDOM(svg.getAttribute('aria-labelledby') === titleId, 'static SVG title mismatch');
  const expectedIds = new Set([titleId, markerId, picker.id, ...Array.from(buttons.values(), (button) => button.id), ...context.nodes.map((node) => node.dom_id)]);
  const idCounts = new Map();
  for (const element of document.querySelectorAll('[id]')) {
    if (expectedIds.has(element.id)) idCounts.set(element.id, (idCounts.get(element.id) ?? 0) + 1);
  }
  requireDOM(Array.from(expectedIds).every((id) => idCounts.get(id) === 1), 'missing or duplicate generated DOM ID');
  const elements = new Map();
  const references = new WeakMap();
  const sceneElements = Array.from(root.querySelectorAll('[data-vizir-scene-id]'));
  requireDOM(sceneElements.length === context.nodes.length, 'scene node count mismatch');
  const byId = new Map(Array.from(svg.querySelectorAll('[id]'), (element) => [element.id, element]));
  requireDOM(byId.get(titleId)?.localName === 'title' && byId.get(titleId).textContent === context.document_id && byId.get(markerId)?.localName === 'marker', 'invalid SVG definitions');
  requireDOM(byId.size === context.nodes.length + 2, 'unexpected SVG DOM identity');
  for (let i = 0; i < context.nodes.length; i += 1) {
    const node = context.nodes[i];
    const element = sceneElements[i];
    requireDOM(svg.contains(element) && generatedOwner(element) === root && element.namespaceURI === SVG_NS && ['g', 'rect', 'circle', 'line', 'path', 'text'].includes(element.localName) && element.id === node.dom_id && element.getAttribute('data-vizir-scene-id') === node.scene_node_id, 'scene element identity mismatch');
    let parent = element.parentElement;
    while (parent && parent !== svg && !parent.hasAttribute('data-vizir-scene-id')) parent = parent.parentElement;
    requireDOM(parent === svg ? node.parent_scene_node_id === null : parent?.getAttribute('data-vizir-scene-id') === node.parent_scene_node_id, 'scene element parent mismatch');
    for (const [attribute, value] of expectedOriginAttributes(node.origin)) {
      requireDOM(element.getAttribute(attribute) === value, 'scene provenance mismatch');
    }
    elements.set(node.scene_node_id, element);
    references.set(element, nodeReference(context, node.scene_node_id));
  }
  for (const element of root.querySelectorAll('[data-hir-node], [data-mir-node], [data-generated-by], [data-key], [data-lineage]')) requireDOM(references.has(element), 'unexpected scene provenance owner');
  // Reject unexpected runtime semantic attributes; opaque identities never form selectors.
  const semanticOwners = new Map([[root, new Set(['data-vizir-explorer', 'data-vizir-instance'])], [viewport, new Set(['data-vizir-viewport'])], [svg, new Set(['data-vizir-canvas'])], [metadata, new Set(['data-vizir-metadata'])], [picker, new Set(['data-vizir-picker'])], [status, new Set(['data-vizir-status'])], [inspector, new Set(['data-vizir-inspector'])]]);
  for (const button of buttons.values()) semanticOwners.set(button, new Set(['data-vizir-action']));
  for (const element of elements.values()) semanticOwners.set(element, new Set(['data-vizir-scene-id']));
  for (const element of [root, ...root.querySelectorAll('*')]) {
    for (const attr of element.attributes) if (attr.name.startsWith('data-vizir-')) requireDOM(semanticOwners.get(element)?.has(attr.name), 'unknown explorer semantic attribute');
  }
  return { document, context, viewport, svg, picker, status, inspector, buttons, elements, references };
}
function saveAttribute(restores, element, name) {
  const original = element.getAttribute(name);
  restores.push(() => original === null ? element.removeAttribute(name) : element.setAttribute(name, original));
}

/** Mount only a complete, validated compiler fragment. No DOM mutation on rejection. */
export function mount(root) {
  const globalObject = root?.ownerDocument?.defaultView ?? globalThis;
  const shared = registry(globalObject);
  if (shared.mounted.has(root)) fail('fragment is already mounted');
  const dom = validateDOM(root);
  const { context, document, viewport, svg, picker, status, inspector, buttons, elements, references } = dom;
  const restores = [];
  const removers = [];
  let state = createState(context);
  let disposed = false;
  let inverse = null;
  let captured = null;
  let renderedSelected = null;
  let renderedInspected = null;
  let pendingClickReference;
  const activePointers = new Set();
  const listen = (target, type, handler, options) => {
    target.addEventListener(type, handler, options);
    removers.push(() => target.removeEventListener(type, handler, options));
  };
  const release = () => {
    const pointer = captured;
    captured = null;
    inverse = null;
    if (pointer !== null) {
      try { if (viewport.hasPointerCapture(pointer)) viewport.releasePointerCapture(pointer); } catch { /* Already released by the browser. */ }
    }
  };
  const inspected = () => state.inspected;
  const label = (ref, selected = false) => `${selected ? 'Selected scene node' : 'Scene node'} ${ref.scene_node_id}`;
  const render = () => {
    const camera = state.camera;
    svg.setAttribute('viewBox', `${camera.x} ${camera.y} ${camera.width} ${camera.height}`);
    root.classList.toggle('vizir-is-panning', state.pan_mode);
    root.classList.toggle('vizir-is-dragging', Boolean(state.gesture?.dragging));
    buttons.get('pan').setAttribute('aria-pressed', String(state.pan_mode));
    buttons.get('zoom-in').disabled = camera.zoom >= 16;
    buttons.get('zoom-out').disabled = camera.zoom <= 1;
    if (renderedSelected !== state.selected) {
      if (renderedSelected) {
        const old = elements.get(renderedSelected.scene_node_id);
        old.classList.remove('vizir-is-selected');
        old.setAttribute('tabindex', '-1');
        old.setAttribute('aria-label', label(renderedSelected));
      }
      if (state.selected) {
        const selected = elements.get(state.selected.scene_node_id);
        selected.classList.add('vizir-is-selected');
        selected.setAttribute('tabindex', '0');
        selected.setAttribute('aria-label', label(state.selected, true));
      }
      renderedSelected = state.selected;
    }
    const inspect = inspected();
    if (renderedInspected !== inspect) {
      if (renderedInspected) elements.get(renderedInspected.scene_node_id).classList.remove('vizir-is-inspected');
      if (inspect) elements.get(inspect.scene_node_id).classList.add('vizir-is-inspected');
      renderedInspected = inspect;
    }
    picker.value = state.selected?.scene_node_id ?? '';
    const nextStatus = `Zoom ${Math.round(camera.zoom * 100)}%. ${state.selected ? `Selected scene node: ${state.selected.scene_node_id}.` : 'No selection.'} ${state.pan_mode ? 'Pan mode.' : 'Inspect mode.'}`;
    if (status.textContent !== nextStatus) status.textContent = nextStatus;
    const nextInspector = inspect ? JSON.stringify({ reference: inspect, origin: contextData(context).index.get(inspect.scene_node_id).origin }, null, 2) : 'Point to an element, focus a selected element, or choose a source element to inspect its provenance.';
    if (inspector.textContent !== nextInspector) inspector.textContent = nextInspector;
  };
  const dispatch = (action) => {
    if (disposed) return;
    const previous = state;
    state = reduce(state, action, context);
    if (previous.gesture && !state.gesture) release();
    if (state !== previous) render();
  };
  const findReference = (target) => {
    let element = target?.nodeType === 1 ? target : target?.parentElement;
    while (element && element !== viewport) {
      if (references.has(element)) return references.get(element);
      element = element.parentElement;
    }
    return null;
  };
  const normalizedPointer = (event) => {
    if (!inverse || !finite(event.clientX) || !finite(event.clientY)) return null;
    const x = inverse.a * event.clientX + inverse.c * event.clientY + inverse.e;
    const y = inverse.b * event.clientX + inverse.d * event.clientY + inverse.f;
    return finite(x) && finite(y) ? { pointer_id: event.pointerId, css_x: event.clientX, css_y: event.clientY, scene_x: x, scene_y: y } : null;
  };
  const cancel = () => dispatch({ type: 'CancelGesture' });
  const api = Object.freeze({
    getState: () => state,
    reset: () => dispatch({ type: 'Reset' }),
    dispose: () => {
      if (disposed) return;
      disposed = true;
      state = cancelled(state);
      activePointers.clear();
      release();
      for (const remove of removers.reverse()) remove();
      for (const restore of restores.reverse()) restore();
      shared.mounted.delete(root);
    },
  });
  try {
    for (const [element, attributes] of [[root, ['class']], [svg, ['viewBox']], [viewport, ['aria-keyshortcuts']], [buttons.get('pan'), ['aria-pressed']], [buttons.get('zoom-in'), ['disabled']], [buttons.get('zoom-out'), ['disabled']]]) for (const attribute of attributes) saveAttribute(restores, element, attribute);
    const originalStatus = status.textContent;
    const originalInspector = inspector.textContent;
    const originalValue = picker.value;
    restores.push(() => { status.textContent = originalStatus; inspector.textContent = originalInspector; picker.value = originalValue; });
    const options = [];
    restores.push(() => { for (const option of options) option.remove(); });
    for (const node of context.nodes) {
      const element = elements.get(node.scene_node_id);
      for (const attribute of ['class', 'tabindex', 'aria-label']) saveAttribute(restores, element, attribute);
      element.setAttribute('tabindex', '-1');
      element.setAttribute('aria-label', label(nodeReference(context, node.scene_node_id)));
      const option = document.createElement('option');
      option.value = node.scene_node_id;
      option.textContent = node.scene_node_id;
      options.push(option);
      picker.appendChild(option);
    }
    viewport.setAttribute('aria-keyshortcuts', 'ArrowLeft ArrowRight ArrowUp ArrowDown + - Home 0 Escape');
    for (const [action, button] of buttons) listen(button, 'click', () => {
      if (action === 'zoom-in' || action === 'zoom-out') dispatch({ type: 'ZoomAt', factor: action === 'zoom-in' ? 1.25 : 1 / 1.25 });
      else dispatch({ type: action === 'reset' ? 'Reset' : 'TogglePan' });
    });
    listen(picker, 'change', () => dispatch({ type: 'Select', reference: picker.value === '' ? null : nodeReference(context, picker.value) }));
    listen(viewport, 'pointerover', (event) => dispatch({ type: 'Hover', reference: findReference(event.target) }));
    listen(viewport, 'pointerout', (event) => dispatch({ type: 'Hover', reference: findReference(event.relatedTarget) }));
    listen(viewport, 'focusin', (event) => dispatch({ type: 'Focus', reference: findReference(event.target) }));
    listen(viewport, 'focusout', (event) => dispatch({ type: 'Focus', reference: findReference(event.relatedTarget) }));
    listen(viewport, 'click', (event) => {
      if (event.button !== 0 || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      const suppressed = state.suppress_click;
      const ref = pendingClickReference === undefined ? findReference(event.target) : pendingClickReference;
      pendingClickReference = undefined;
      dispatch({ type: 'Click', reference: ref });
      if (!suppressed && ref) elements.get(ref.scene_node_id).focus({ preventScroll: true });
    });
    listen(viewport, 'keydown', (event) => {
      if (event.target !== viewport) return;
      const action = viewportKeyAction(event);
      if (action) { event.preventDefault(); dispatch(action); }
    });
    listen(viewport, 'pointerdown', (event) => {
      if (state.gesture && event.pointerId !== state.gesture.pointer_id) { cancel(); return; }
      if (activePointers.size > 1 || event.button !== 0 || event.isPrimary === false || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      pendingClickReference = undefined;
      dispatch({ type: 'PrepareClick' });
      if (!state.pan_mode) return;
      pendingClickReference = findReference(event.target);
      try {
        const matrix = svg.getScreenCTM();
        requireDOM(matrix && [matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f].every(finite) && matrix.a * matrix.d - matrix.b * matrix.c !== 0, 'SVG coordinates are unavailable');
        const inverted = matrix.inverse();
        inverse = { a: inverted.a, b: inverted.b, c: inverted.c, d: inverted.d, e: inverted.e, f: inverted.f };
        const pointer = normalizedPointer(event);
        requireDOM(pointer, 'pointer coordinates are unavailable');
        dispatch({ type: 'PointerStart', ...pointer });
        if (!state.gesture) { inverse = null; return; }
        captured = event.pointerId;
        viewport.setPointerCapture(event.pointerId);
        event.preventDefault();
      } catch {
        cancel();
        release();
        status.textContent = 'Pan could not start because pointer capture or SVG coordinates are unavailable. Use the viewport arrow keys instead.';
      }
    });
    listen(viewport, 'pointermove', (event) => {
      if (!state.gesture || event.pointerId !== state.gesture.pointer_id) return;
      const pointer = normalizedPointer(event);
      if (!pointer || event.buttons !== 1) { cancel(); return; }
      dispatch({ type: 'PointerMove', ...pointer });
    });
    listen(viewport, 'pointerup', (event) => {
      if (state.gesture?.pointer_id !== event.pointerId) return;
      const pointer = normalizedPointer(event);
      if (pointer) dispatch({ type: 'PointerMove', ...pointer });
      dispatch({ type: 'PointerEnd', pointer_id: event.pointerId });
    });
    listen(viewport, 'pointercancel', cancel);
    listen(viewport, 'lostpointercapture', (event) => { if (state.gesture?.pointer_id === event.pointerId) cancel(); });
    listen(globalObject, 'blur', () => { activePointers.clear(); cancel(); });
    listen(globalObject, 'pointerdown', (event) => {
      activePointers.add(event.pointerId);
      if (state.gesture && event.pointerId !== state.gesture.pointer_id) cancel();
    }, true);
    listen(globalObject, 'pointerup', (event) => activePointers.delete(event.pointerId), true);
    listen(globalObject, 'pointercancel', (event) => activePointers.delete(event.pointerId), true);
    listen(globalObject, 'keydown', (event) => { if (event.key === 'Escape' && state.gesture && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) { event.preventDefault(); cancel(); } }, true);
    render();
    shared.mounted.set(root, api);
    return api;
  } catch (error) {
    api.dispose();
    throw error;
  }
}

/** Repeated execution shares a versioned registry and at most one ready listener. */
export function bootstrap(globalObject = globalThis) {
  if (!globalObject.document) return null;
  const shared = registry(globalObject);
  if (!shared.api) shared.api = Object.freeze({ version: FORMAT, profile: PROFILE, runtime_payload_sha256: PAYLOAD_SHA256, mount, bootstrap });
  if (!own(globalObject, 'VizIRExplorerV1')) Object.defineProperty(globalObject, 'VizIRExplorerV1', { value: shared.api, writable: false, configurable: false });
  else if (globalObject.VizIRExplorerV1 !== shared.api) fail('global runtime API collision');
  const run = () => {
    for (const root of globalObject.document.querySelectorAll('[data-vizir-explorer]')) {
      if (shared.mounted.has(root)) continue;
      try { shared.api.mount(root); }
      catch (error) {
        const status = root.querySelector('[data-vizir-status]');
        if (status && generatedOwner(status) === root) status.textContent = `Explorer unavailable. Static visualization remains visible. ${error instanceof Error ? error.message : 'Invalid fragment.'}`;
      }
    }
  };
  if (globalObject.document.readyState === 'loading') {
    if (!shared.listening.has(globalObject.document)) {
      shared.listening.add(globalObject.document);
      globalObject.document.addEventListener('DOMContentLoaded', run, { once: true });
    }
  } else run();
  return shared.api;
}
if (typeof globalThis.document !== 'undefined') {
  try { bootstrap(globalThis); }
  catch (error) {
    for (const root of globalThis.document.querySelectorAll('[data-vizir-explorer]')) {
      const status = root.querySelector('[data-vizir-status]');
      if (status && generatedOwner(status) === root) status.textContent = `Explorer initialization rejected. Visualization remains visible. ${error instanceof Error ? error.message : 'Runtime incompatibility.'}`;
    }
  }
}
