'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {
  TERRAIN_VERSION,
  adaptTerrainClassification,
  adaptTerrainReport,
  assertTerrainReport,
  terrainClassOverlay,
  terrainClassificationScript,
  terrainFilterClassEvidence,
  terrainEndpoint,
  terrainLayout
} = require('../positorium-terrain.js');

const report = {
  terrain_version: 1,
  resolved_as_of: "'2026-01-01'",
  database: { referenced_things: 9, roles: 5, appearances: 4, appearance_sets: 3, posits: 4 },
  projection: {
    complete: true,
    total_attribute_roles: 2,
    roles: [
      { id: '6', name: 'name', bit: 0, history_support: 2, current_support: 2 },
      { id: '7', name: 'RFID', bit: 1, history_support: 1, current_support: 1 }
    ]
  },
  relationship_catalog: {
    complete: true,
    total_signatures: 1,
    default_signature_id: 'terrain-v1-signature-8-9',
    signatures: [{
      id: 'terrain-v1-signature-8-9',
      roles: [{ id: '8', name: 'owner' }, { id: '9', name: 'pet' }]
    }]
  },
  frames: {
    history: {
      scope: 'history',
      stats: { endpoint_things: 3, roles: 4, appearance_sets: 3, posits: 4, incidences: 6 },
      role_supports: [{ role_id: '6', distinct_things: 2 }, { role_id: '7', distinct_things: 1 }],
      profiles: [
        { id: 'terrain-v1-profile-000', mask: 0, present_role_ids: [], absent_role_ids: ['6', '7'], things: 1, isopleth_id: null },
        { id: 'terrain-v1-profile-001', mask: 1, present_role_ids: ['6'], absent_role_ids: ['7'], things: 2, isopleth_id: 'terrain-v1-isopleth-001' }
      ],
      isopleths: [{ id: 'terrain-v1-isopleth-001', mask: 1, included_role_ids: ['6'], support: 2 }],
      relationships: [{
        signature_id: 'terrain-v1-signature-8-9', appearance_sets: 1, posits: 2,
        role_totals: [
          { role_id: '8', distinct_things: 1, participations: 1 },
          { role_id: '9', distinct_things: 1, participations: 1 }
        ],
        allocations: [
          { id: 'a-zero', role_id: '8', profile_id: 'terrain-v1-profile-000', profile_mask: 0, isopleth_id: null, distinct_things: 1, participations: 1 },
          { id: 'a-one', role_id: '9', profile_id: 'terrain-v1-profile-001', profile_mask: 1, isopleth_id: 'terrain-v1-isopleth-001', distinct_things: 1, participations: 1 }
        ]
      }]
    },
    current: {
      scope: 'current',
      stats: { endpoint_things: 3, roles: 4, appearance_sets: 3, posits: 3, incidences: 5 },
      role_supports: [{ role_id: '6', distinct_things: 2 }, { role_id: '7', distinct_things: 1 }],
      profiles: [
        { id: 'terrain-v1-profile-000', mask: 0, present_role_ids: [], absent_role_ids: ['6', '7'], things: 1, isopleth_id: null },
        { id: 'terrain-v1-profile-001', mask: 1, present_role_ids: ['6'], absent_role_ids: ['7'], things: 2, isopleth_id: 'terrain-v1-isopleth-001' }
      ],
      isopleths: [{ id: 'terrain-v1-isopleth-001', mask: 1, included_role_ids: ['6'], support: 2 }],
      relationships: [{
        signature_id: 'terrain-v1-signature-8-9', appearance_sets: 1, posits: 1,
        role_totals: [
          { role_id: '8', distinct_things: 1, participations: 1 },
          { role_id: '9', distinct_things: 1, participations: 1 }
        ],
        allocations: [
          { id: 'a-zero', role_id: '8', profile_id: 'terrain-v1-profile-000', profile_mask: 0, isopleth_id: null, distinct_things: 1, participations: 1 },
          { id: 'a-one', role_id: '9', profile_id: 'terrain-v1-profile-001', profile_mask: 1, isopleth_id: 'terrain-v1-isopleth-001', distinct_things: 1, participations: 1 }
        ]
      }]
    }
  }
};

assert.equal(TERRAIN_VERSION, 1);
assert.equal(assertTerrainReport(report), report);
assert.throws(() => assertTerrainReport({ terrain_version: 2 }), /Unsupported Terrain report version/);
assert.equal(terrainEndpoint('http://127.0.0.1:3000/v1/query'), 'http://127.0.0.1:3000/v1/terrain');

const httpData = adaptTerrainReport(structuredClone(report));
const wasmData = adaptTerrainReport(structuredClone(report));
assert.deepEqual(httpData, wasmData, 'HTTP and WASM reports render through the same adapter');
assert.equal(httpData.frames.current.label, "Maximal values as of '2026-01-01'");
assert.equal(httpData.frames.history.projection.roles[0].distinct_things, 2);
assert.equal(httpData.frames.history.relationships[0].roles.join(','), 'owner,pet');
assert.equal(httpData.frames.history.relationships[0].role_totals[0].role, 'owner');
assert.equal(httpData.frames.history.relationships[0].allocations[1].role, 'pet');
assert.equal(httpData.frames.history.relationships[0].allocations[0].profile_mask, 0);
assert.equal(httpData.frames.history.relationships[0].allocations[0].isopleth_id, null);

const goldenTopology = {
  projection: {
    roles: [
      { id: '6', name: 'name' },
      { id: '7', name: 'hair color' },
      { id: '8', name: 'height' },
      { id: '9', name: 'social security number' },
      { id: '10', name: 'beard color' },
      { id: '11', name: 'RFID' }
    ]
  }
};
const goldenIsopleths = [
  { id: 'core', included_roles: ['6', '7'], support: 6 },
  { id: 'rfid', included_roles: ['6', '7', '11'], support: 2 },
  { id: 'identity', included_roles: ['6', '7', '8', '9'], support: 3 },
  { id: 'beard', included_roles: ['6', '7', '8', '9', '10'], support: 1 }
];
const goldenAllocations = [
  { id: 'owner-beard', isopleth_id: 'beard' },
  { id: 'owner-identity', isopleth_id: 'identity' },
  { id: 'pet-rfid', isopleth_id: 'rfid' }
];
const goldenLayout = terrainLayout(goldenTopology, goldenIsopleths, goldenAllocations);
assert.deepEqual(goldenLayout, terrainLayout(goldenTopology, goldenIsopleths, goldenAllocations), 'layout must be deterministic');
assert.equal(goldenLayout.isopleths.identity.parent_id, 'core');
assert.equal(goldenLayout.isopleths.beard.parent_id, 'identity');
assert.equal(goldenLayout.isopleths.rfid.parent_id, 'core');
assert.ok(goldenLayout.roles['8'].position[0] < goldenLayout.roles['6'].position[0], 'identity-only Roles branch left of the core');
assert.ok(goldenLayout.roles['11'].position[0] > goldenLayout.roles['6'].position[0], 'RFID branches right of the core');
assert.ok(goldenLayout.roles['10'].position[0] < goldenLayout.roles['8'].position[0], 'nested additions extend their branch');
assert.ok(goldenLayout.roles['10'].position[1] < goldenLayout.roles['8'].position[1], 'nested additions fan away from their parent');
assert.ok(Object.values(goldenLayout.isopleths).every(isopleth => /^M.*Q.*Z$/.test(isopleth.path)), 'isopleths use smooth hulls');
assert.ok(Object.values(goldenLayout.roles).every(role => role.position.every(Number.isFinite)), 'every projected Role is positioned');
assert.ok(goldenLayout.relationship.label[1] > Math.max(...Object.values(goldenLayout.roles).map(role => role.position[1])), 'relationship fan sits below the role topology');

const classificationScript = terrainClassificationScript("'2026-01-01'");
assert.match(classificationScript, /in effect '2026-01-01', '2026-01-01'/);
assert.match(classificationScript, /via \?assertion/);
assert.match(classificationScript, /\(\?member, thing\), \(\?class, class\)/);
assert.doesNotMatch(classificationScript, /subclass/);

const cell = (kind, text) => ({ kind, text });
const classification = adaptTerrainClassification([
  {
    columns: ['classification', 'member', 'class', 'state', 'appeared', 'assertion', 'source', 'certainty', 'asserted'],
    rows: [
      [cell('posit', '100'), cell('thing', '20'), cell('thing', '30'), cell('literal', '"active"'), cell('time', '2025-01-01'), cell('posit', '200'), cell('thing', '40'), cell('literal', '80%'), cell('time', '2025-01-02')],
      [cell('posit', '101'), cell('thing', '21'), cell('thing', '30'), cell('literal', '"active"'), cell('time', '2025-01-01'), cell('posit', '201'), cell('thing', '41'), cell('literal', '90%'), cell('time', '2025-01-02')],
      [cell('posit', '102'), cell('thing', '22'), cell('thing', '30'), cell('literal', '"active"'), cell('time', '2025-01-01'), cell('posit', '202'), cell('thing', '40'), cell('literal', '-70%'), cell('time', '2025-01-02')]
    ]
  },
  {
    columns: ['member', 'class', 'state', 'source', 'certainty', 'member_role'],
    rows: [
      ['20', '30', '"active"', '40', '80%', 'name'],
      ['21', '30', '"active"', '41', '90%', 'name']
    ]
  }
]);
assert.deepEqual(classification.roles_by_member, { '20': ['name'], '21': ['name'] });
assert.equal(terrainFilterClassEvidence(classification, {
  class_id: '30', value: '"active"', source: 'all', certainty: 'positive'
}).length, 2, 'negative opposition is not shaded by the positive-support policy');
assert.equal(terrainFilterClassEvidence(classification, {
  class_id: '30', value: '"active"', source: '40', certainty: 'nonzero'
}).length, 2, 'the visible source and certainty policies are applied client-side');

const currentLayout = terrainLayout(
  httpData.frames.current,
  httpData.frames.current.isopleths,
  httpData.frames.current.relationships[0].allocations
);
const fullClassOverlay = terrainClassOverlay(currentLayout, httpData.frames.current, classification, {
  class_id: '30', value: '"active"', source: 'all', certainty: 'positive'
});
assert.deepEqual(fullClassOverlay.members, ['20', '21']);
assert.equal(fullClassOverlay.regions.length, 1);
assert.equal(fullClassOverlay.regions[0].kind, 'full-profile');
assert.equal(fullClassOverlay.regions[0].path, currentLayout.isopleths['terrain-v1-isopleth-001'].path, 'a complete classified profile reuses the isopleth interior exactly');

const partialClassOverlay = terrainClassOverlay(currentLayout, httpData.frames.current, classification, {
  class_id: '30', value: '"active"', source: '40', certainty: 'positive'
});
assert.deepEqual(partialClassOverlay.members, ['20']);
assert.equal(partialClassOverlay.regions[0].kind, 'member-group');
assert.match(partialClassOverlay.regions[0].path, /^M.*A.*Z$/, 'a partial profile receives padded member geometry rather than shading the complete isopleth');

const clientSource = fs.readFileSync(path.join(__dirname, '..', 'positorium-terrain.js'), 'utf8');
const studioSource = fs.readFileSync(path.join(__dirname, '..', 'positorium.html'), 'utf8');
const serverSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'server.rs'), 'utf8');
const studioVersion = studioSource.match(/data-version="([^"]+)"/)?.[1];
assert.ok(studioVersion, 'Studio version must be declared');
assert.ok(studioSource.includes(`positorium-terrain.css?v=${studioVersion}`), 'Terrain CSS must follow the Studio cache version');
assert.ok(studioSource.includes(`positorium-terrain.js?v=${studioVersion}`), 'Terrain JavaScript must follow the Studio cache version');
for (const removed of [
  'buildTerrainData', 'captureTerrainResultSets', 'TERRAIN_REQUIRED_COLUMNS',
  'normalizeTerrainRows', 'terrainHash', 'Query data'
]) {
  assert.equal(clientSource.includes(removed) || studioSource.includes(removed), false, `${removed} must be removed`);
}
assert.match(studioSource, /refreshTerrain/);
assert.match(studioSource, /id="terrainClass"/);
assert.match(studioSource, /id="terrainClassValue"/);
assert.match(studioSource, /id="terrainClassSource"/);
assert.match(clientSource, /terrainClassificationScript/);
assert.match(clientSource, /All effective sources \(union\)/);
assert.match(clientSource, /engine\.terrain/);
assert.match(clientSource, /does not implement Terrain contract 1/);
assert.match(clientSource, /Database snapshot is empty/);
assert.doesNotMatch(clientSource, /streamMode/);
assert.match(serverSource, /route\("\/v1\/terrain", post\(terrain\)\)/);
assert.doesNotMatch(serverSource, /\/v1\/terrain\/(?:stream|events)/);

console.log('authoritative Terrain client adapter: ok');
