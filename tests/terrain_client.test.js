'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {
  TERRAIN_VERSION,
  adaptTerrainReport,
  assertTerrainReport,
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
      { id: '3', name: 'name', bit: 0, history_support: 2, current_support: 2 },
      { id: '4', name: 'RFID', bit: 1, history_support: 1, current_support: 1 }
    ]
  },
  relationship_catalog: {
    complete: true,
    total_signatures: 1,
    default_signature_id: 'terrain-v1-signature-5-6',
    signatures: [{
      id: 'terrain-v1-signature-5-6',
      roles: [{ id: '5', name: 'owner' }, { id: '6', name: 'pet' }]
    }]
  },
  frames: {
    history: {
      scope: 'history',
      stats: { endpoint_things: 3, roles: 4, appearance_sets: 3, posits: 4, incidences: 6 },
      role_supports: [{ role_id: '3', distinct_things: 2 }, { role_id: '4', distinct_things: 1 }],
      profiles: [
        { id: 'terrain-v1-profile-000', mask: 0, present_role_ids: [], absent_role_ids: ['3', '4'], things: 1, isopleth_id: null },
        { id: 'terrain-v1-profile-001', mask: 1, present_role_ids: ['3'], absent_role_ids: ['4'], things: 2, isopleth_id: 'terrain-v1-isopleth-001' }
      ],
      isopleths: [{ id: 'terrain-v1-isopleth-001', mask: 1, included_role_ids: ['3'], support: 2 }],
      relationships: [{
        signature_id: 'terrain-v1-signature-5-6', appearance_sets: 1, posits: 2,
        role_totals: [
          { role_id: '5', distinct_things: 1, participations: 1 },
          { role_id: '6', distinct_things: 1, participations: 1 }
        ],
        allocations: [
          { id: 'a-zero', role_id: '5', profile_id: 'terrain-v1-profile-000', profile_mask: 0, isopleth_id: null, distinct_things: 1, participations: 1 },
          { id: 'a-one', role_id: '6', profile_id: 'terrain-v1-profile-001', profile_mask: 1, isopleth_id: 'terrain-v1-isopleth-001', distinct_things: 1, participations: 1 }
        ]
      }]
    },
    current: {
      scope: 'current',
      stats: { endpoint_things: 3, roles: 4, appearance_sets: 3, posits: 3, incidences: 5 },
      role_supports: [{ role_id: '3', distinct_things: 2 }, { role_id: '4', distinct_things: 1 }],
      profiles: [
        { id: 'terrain-v1-profile-000', mask: 0, present_role_ids: [], absent_role_ids: ['3', '4'], things: 1, isopleth_id: null },
        { id: 'terrain-v1-profile-001', mask: 1, present_role_ids: ['3'], absent_role_ids: ['4'], things: 2, isopleth_id: 'terrain-v1-isopleth-001' }
      ],
      isopleths: [{ id: 'terrain-v1-isopleth-001', mask: 1, included_role_ids: ['3'], support: 2 }],
      relationships: [{
        signature_id: 'terrain-v1-signature-5-6', appearance_sets: 1, posits: 1,
        role_totals: [
          { role_id: '5', distinct_things: 1, participations: 1 },
          { role_id: '6', distinct_things: 1, participations: 1 }
        ],
        allocations: [
          { id: 'a-zero', role_id: '5', profile_id: 'terrain-v1-profile-000', profile_mask: 0, isopleth_id: null, distinct_things: 1, participations: 1 },
          { id: 'a-one', role_id: '6', profile_id: 'terrain-v1-profile-001', profile_mask: 1, isopleth_id: 'terrain-v1-isopleth-001', distinct_things: 1, participations: 1 }
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
      { id: '3', name: 'name' },
      { id: '4', name: 'hair color' },
      { id: '5', name: 'height' },
      { id: '6', name: 'social security number' },
      { id: '7', name: 'beard color' },
      { id: '8', name: 'RFID' }
    ]
  }
};
const goldenIsopleths = [
  { id: 'core', included_roles: ['3', '4'], support: 6 },
  { id: 'rfid', included_roles: ['3', '4', '8'], support: 2 },
  { id: 'identity', included_roles: ['3', '4', '5', '6'], support: 3 },
  { id: 'beard', included_roles: ['3', '4', '5', '6', '7'], support: 1 }
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
assert.ok(goldenLayout.roles['5'].position[0] < goldenLayout.roles['3'].position[0], 'identity-only Roles branch left of the core');
assert.ok(goldenLayout.roles['8'].position[0] > goldenLayout.roles['3'].position[0], 'RFID branches right of the core');
assert.ok(goldenLayout.roles['7'].position[0] < goldenLayout.roles['5'].position[0], 'nested additions extend their branch');
assert.ok(goldenLayout.roles['7'].position[1] < goldenLayout.roles['5'].position[1], 'nested additions fan away from their parent');
assert.ok(Object.values(goldenLayout.isopleths).every(isopleth => /^M.*Q.*Z$/.test(isopleth.path)), 'isopleths use smooth hulls');
assert.ok(Object.values(goldenLayout.roles).every(role => role.position.every(Number.isFinite)), 'every projected Role is positioned');
assert.ok(goldenLayout.relationship.label[1] > Math.max(...Object.values(goldenLayout.roles).map(role => role.position[1])), 'relationship fan sits below the role topology');

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
assert.match(clientSource, /engine\.terrain/);
assert.match(clientSource, /does not implement Terrain contract 1/);
assert.match(clientSource, /Database snapshot is empty/);
assert.doesNotMatch(clientSource, /streamMode/);
assert.match(serverSource, /route\("\/v1\/terrain", post\(terrain\)\)/);
assert.doesNotMatch(serverSource, /\/v1\/terrain\/(?:stream|events)/);

console.log('authoritative Terrain client adapter: ok');
