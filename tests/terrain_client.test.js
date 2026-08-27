'use strict';

const assert = require('node:assert/strict');
const { buildTerrainData } = require('../positorium-terrain.js');

const columns = ['posit_id', 'appearance_set', 'thing', 'role_name', 'value', 'time'];
const cell = (kind, text) => ({ kind, text });
const rows = [];

function addPosit(set, posit, appearances, value = 'value') {
  appearances.forEach(([thing, role]) => {
    rows.push([
      cell('posit', posit),
      cell('appearance_set', set),
      cell('thing', thing),
      cell('role', role),
      cell('literal', value),
      cell('time', '2024-01-01')
    ]);
  });
}

addPosit('set-t1-name', 'p1', [['t1', 'name']]);
addPosit('set-t1-hair', 'p2', [['t1', 'hair color']]);
addPosit('set-t2-name', 'p3', [['t2', 'name']]);
addPosit('set-t2-hair', 'p4', [['t2', 'hair color']]);
addPosit('set-t2-rfid', 'p5', [['t2', 'RFID']]);
addPosit('set-owner-pet', 'p6', [['t1', 'owner'], ['t2', 'pet']], 'fostered');
addPosit('set-owner-pet', 'p7', [['t1', 'owner'], ['t2', 'pet']], 'adopted');

const snapshotRows = rows.filter(row => row[0].text !== 'p6');
const data = buildTerrainData([
  { columns, rows, row_count: rows.length, search: 'search incidence' },
  { columns, rows: snapshotRows, row_count: snapshotRows.length, search: 'search incidence as of @NOW' }
]);

assert.ok(data);
assert.equal(data.source, 'query_results');
assert.deepEqual(data.result_rows, { history: 9, snapshot: 7 });

const history = data.frames.history;
const snapshot = data.frames.snapshot;
assert.deepEqual(history.stats, {
  things: 2,
  roles: 5,
  appearance_sets: 6,
  posits: 7,
  rows: 9
});
assert.equal(snapshot.stats.posits, 6);
assert.equal(history.projection.complete, true);
assert.equal(history.projection.roles.length, 3);
assert.deepEqual(history.isopleths.map(isopleth => isopleth.support).sort((a, b) => b - a), [2, 1]);
assert.deepEqual(history.relationship.roles, ['owner', 'pet']);
assert.equal(history.relationship.appearance_sets, 1);
assert.equal(history.relationship.posits, 2);
assert.equal(snapshot.relationship.posits, 1);
assert.equal(history.relationship.allocations.length, 2);
assert.deepEqual(
  history.relationship.allocations.map(allocation => [allocation.role, allocation.distinct_things, allocation.participations]),
  [['owner', 1, 1], ['pet', 1, 1]]
);

console.log('terrain client aggregation: ok');
