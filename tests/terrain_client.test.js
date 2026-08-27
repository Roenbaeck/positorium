'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { TERRAIN_VERSION, adaptTerrainReport, assertTerrainReport, terrainEndpoint } = require('../positorium-terrain.js');

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

const clientSource = fs.readFileSync(path.join(__dirname, '..', 'positorium-terrain.js'), 'utf8');
const studioSource = fs.readFileSync(path.join(__dirname, '..', 'positorium.html'), 'utf8');
const serverSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'server.rs'), 'utf8');
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
