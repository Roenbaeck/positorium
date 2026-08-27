const TERRAIN_REQUIRED_COLUMNS = ['posit', 'set', 'thing', 'role', 'value', 'time'];
const TERRAIN_COLUMN_ALIASES = {
  posit: ['posit', 'posit_id', 'p'],
  set: ['set', 'appearance_set', 'aset'],
  thing: ['thing'],
  role: ['role', 'role_name', 'r'],
  value: ['value', 'v'],
  time: ['time', 't']
};
const TERRAIN_MAX_ROLES = 8;
const TERRAIN_ROLE_POSITIONS = [
  [356, 155], [356, 229], [205, 222], [205, 302],
  [85, 85], [655, 270], [520, 95], [575, 390]
];
const TERRAIN_TONES = ['sea', 'leaf', 'earth', 'berry'];

let terrainData = null;
const terrainState = {
  scope: 'snapshot',
  minimumSupport: 0,
  relationships: true,
  selectedType: null,
  selectedId: null
};

function terrainCellText(cell) {
  if (cell && typeof cell === 'object' && Object.prototype.hasOwnProperty.call(cell, 'text')) {
    return String(cell.text);
  }
  return cell === null || cell === undefined ? '' : String(cell);
}

function terrainResultSetColumns(resultSet) {
  return Array.isArray(resultSet?.columns)
    ? resultSet.columns.map(column => String(column).replace(/^\?/, '').toLowerCase())
    : [];
}

function isTerrainIncidenceResultSet(resultSet) {
  const columns = terrainResultSetColumns(resultSet);
  return Array.isArray(resultSet?.rows)
    && TERRAIN_REQUIRED_COLUMNS.every(column => TERRAIN_COLUMN_ALIASES[column].some(alias => columns.includes(alias)));
}

function normalizeTerrainRows(resultSet) {
  const columns = terrainResultSetColumns(resultSet);
  const indexes = Object.fromEntries(TERRAIN_REQUIRED_COLUMNS.map(column => [
    column,
    columns.findIndex(candidate => TERRAIN_COLUMN_ALIASES[column].includes(candidate))
  ]));
  return resultSet.rows.map(row => Object.fromEntries(
    TERRAIN_REQUIRED_COLUMNS.map(column => [column, terrainCellText(row[indexes[column]])])
  ));
}

function terrainHash(value) {
  let hash = 2166136261;
  for (const character of String(value)) {
    hash ^= character.codePointAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

function terrainId(prefix, value) {
  const slug = String(value)
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 28) || 'item';
  return `${prefix}-${slug}-${terrainHash(value)}`;
}

function terrainSetKey(values) {
  return [...values].sort().join('\u0001');
}

function terrainAppearanceRecords(rows) {
  const records = new Map();
  rows.forEach(row => {
    if (!records.has(row.set)) {
      records.set(row.set, { id: row.set, appearances: new Map(), posits: new Set() });
    }
    const record = records.get(row.set);
    record.appearances.set(`${row.role}\u0000${row.thing}`, { role: row.role, thing: row.thing });
    record.posits.add(row.posit);
  });
  return records;
}

function terrainRelationship(records, profilesByThing, profiles) {
  const signatures = new Map();
  records.forEach(record => {
    if (record.appearances.size < 2) return;
    const roles = [...new Set([...record.appearances.values()].map(appearance => appearance.role))].sort();
    if (roles.length < 2) return;
    const key = terrainSetKey(roles);
    if (!signatures.has(key)) signatures.set(key, { roles, records: [] });
    signatures.get(key).records.push(record);
  });

  const selected = [...signatures.values()].sort((left, right) =>
    right.records.length - left.records.length
      || right.roles.length - left.roles.length
      || left.roles.join('\u0001').localeCompare(right.roles.join('\u0001'))
  )[0];
  if (!selected) return null;

  const profileById = new Map(profiles.map(profile => [profile.id, profile]));
  const roleTotals = new Map(selected.roles.map(role => [role, { role, things: new Set(), participations: 0 }]));
  const allocations = new Map();
  const posits = new Set();

  selected.records.forEach(record => {
    record.posits.forEach(posit => posits.add(posit));
    record.appearances.forEach(appearance => {
      const total = roleTotals.get(appearance.role);
      if (!total) return;
      total.things.add(appearance.thing);
      total.participations += 1;

      const profileId = profilesByThing.get(appearance.thing);
      const profile = profileById.get(profileId);
      if (!profile?.isopleth_id) return;
      const key = `${appearance.role}\u0000${profileId}`;
      if (!allocations.has(key)) {
        allocations.set(key, {
          id: terrainId('allocation', key),
          role: appearance.role,
          profile_id: profileId,
          isopleth_id: profile.isopleth_id,
          things: new Set(),
          participations: 0
        });
      }
      const allocation = allocations.get(key);
      allocation.things.add(appearance.thing);
      allocation.participations += 1;
    });
  });

  return {
    id: terrainId('relationship', selected.roles.join('|')),
    roles: selected.roles,
    appearance_sets: selected.records.length,
    posits: posits.size,
    role_totals: [...roleTotals.values()].map(total => ({
      role: total.role,
      distinct_things: total.things.size,
      participations: total.participations
    })),
    allocations: [...allocations.values()].map(allocation => ({
      id: allocation.id,
      role: allocation.role,
      profile_id: allocation.profile_id,
      isopleth_id: allocation.isopleth_id,
      distinct_things: allocation.things.size,
      participations: allocation.participations
    })).sort((left, right) => left.role.localeCompare(right.role)
      || right.distinct_things - left.distinct_things
      || left.profile_id.localeCompare(right.profile_id))
  };
}

function buildTerrainFrame(resultSet, label) {
  const rows = normalizeTerrainRows(resultSet);
  const records = terrainAppearanceRecords(rows);
  const allThings = new Set();
  const allRoles = new Set();
  const allPosits = new Set();
  const attributeRoleThings = new Map();

  records.forEach(record => {
    record.posits.forEach(posit => allPosits.add(posit));
    record.appearances.forEach(appearance => {
      allThings.add(appearance.thing);
      allRoles.add(appearance.role);
    });
    if (record.appearances.size !== 1) return;
    const appearance = record.appearances.values().next().value;
    if (!attributeRoleThings.has(appearance.role)) attributeRoleThings.set(appearance.role, new Set());
    attributeRoleThings.get(appearance.role).add(appearance.thing);
  });

  const projectedRoleNames = [...attributeRoleThings.entries()]
    .sort((left, right) => right[1].size - left[1].size || left[0].localeCompare(right[0]))
    .slice(0, TERRAIN_MAX_ROLES)
    .map(([role]) => role);
  const projectedRoles = projectedRoleNames.map(role => ({
    id: terrainId('role', role),
    name: role,
    distinct_things: attributeRoleThings.get(role).size
  }));
  const roleIdByName = new Map(projectedRoles.map(role => [role.name, role.id]));
  const projectedRoleOrder = new Map(projectedRoles.map((role, index) => [role.id, index]));

  const thingRoleIds = new Map([...allThings].map(thing => [thing, new Set()]));
  records.forEach(record => {
    if (record.appearances.size !== 1) return;
    const appearance = record.appearances.values().next().value;
    const roleId = roleIdByName.get(appearance.role);
    if (roleId) thingRoleIds.get(appearance.thing)?.add(roleId);
  });

  const profileGroups = new Map();
  thingRoleIds.forEach((roleIds, thing) => {
    const presentRoles = [...roleIds].sort((left, right) => projectedRoleOrder.get(left) - projectedRoleOrder.get(right));
    const key = terrainSetKey(presentRoles);
    if (!profileGroups.has(key)) profileGroups.set(key, { present_roles: presentRoles, things: new Set() });
    profileGroups.get(key).things.add(thing);
  });

  const profilesByThing = new Map();
  const profiles = [...profileGroups.entries()].map(([key, group]) => {
    const id = terrainId('profile', key || 'empty');
    group.things.forEach(thing => profilesByThing.set(thing, id));
    return {
      id,
      present_roles: group.present_roles,
      absent_roles: projectedRoles.map(role => role.id).filter(roleId => !group.present_roles.includes(roleId)),
      things: group.things.size,
      isopleth_id: group.present_roles.length ? terrainId('isopleth', key) : null
    };
  }).sort((left, right) => right.things - left.things || left.id.localeCompare(right.id));

  const isopleths = profiles
    .filter(profile => profile.present_roles.length)
    .map(profile => ({
      id: profile.isopleth_id,
      included_roles: profile.present_roles,
      support: profiles
        .filter(candidate => profile.present_roles.every(roleId => candidate.present_roles.includes(roleId)))
        .reduce((total, candidate) => total + candidate.things, 0)
    }))
    .sort((left, right) => left.included_roles.length - right.included_roles.length
      || right.support - left.support
      || left.id.localeCompare(right.id));

  return {
    label,
    stats: {
      things: allThings.size,
      roles: allRoles.size,
      appearance_sets: records.size,
      posits: allPosits.size,
      rows: rows.length
    },
    projection: {
      complete: attributeRoleThings.size <= TERRAIN_MAX_ROLES,
      total_roles: attributeRoleThings.size,
      roles: projectedRoles
    },
    profiles,
    isopleths,
    relationship: terrainRelationship(records, profilesByThing, profiles)
  };
}

function buildTerrainData(resultSets) {
  const compatible = (Array.isArray(resultSets) ? resultSets : []).filter(isTerrainIncidenceResultSet);
  if (compatible.length < 2) return null;

  const snapshot = compatible.find(resultSet => /\bas\s+of\b/i.test(String(resultSet.search || '')))
    || compatible[compatible.length - 1];
  const history = compatible.find(resultSet => resultSet !== snapshot && !/\bas\s+of\b/i.test(String(resultSet.search || '')))
    || compatible.find(resultSet => resultSet !== snapshot);
  if (!history || history === snapshot) return null;

  const historyFrame = buildTerrainFrame(history, 'All recorded history');
  const snapshotFrame = buildTerrainFrame(snapshot, 'Maximal values as of now');
  return {
    schema_version: 3,
    source: 'query_results',
    database: historyFrame.stats,
    frames: { history: historyFrame, snapshot: snapshotFrame },
    result_rows: { history: historyFrame.stats.rows, snapshot: snapshotFrame.stats.rows }
  };
}

function formatTerrainCount(value) {
  return new Intl.NumberFormat().format(value);
}

function terrainFrame() {
  return terrainData?.frames[terrainState.scope] || null;
}

function terrainRole(roleId) {
  return terrainFrame()?.projection.roles.find(role => role.id === roleId);
}

function terrainRoleNames(roleIds) {
  return roleIds.map(roleId => terrainRole(roleId)?.name || roleId);
}

function terrainProfile(profileId) {
  return terrainFrame()?.profiles.find(profile => profile.id === profileId);
}

function terrainSelection() {
  const frame = terrainFrame();
  if (!frame) return null;
  if (terrainState.selectedType === 'relationship') return frame.relationship;
  if (terrainState.selectedType === 'allocation') {
    return frame.relationship?.allocations.find(allocation => allocation.id === terrainState.selectedId) || null;
  }
  if (terrainState.selectedType === 'role') {
    return frame.projection.roles.find(role => role.id === terrainState.selectedId) || null;
  }
  return frame.isopleths.find(isopleth => isopleth.id === terrainState.selectedId) || null;
}

function selectTerrainItem(type, id) {
  terrainState.selectedType = type;
  terrainState.selectedId = id;
  renderTerrain();
}

function syncTerrainControls() {
  const frame = terrainFrame();
  const maximumSupport = frame ? Math.max(1, ...frame.isopleths.map(isopleth => isopleth.support)) : 1;
  els.terrainSupport.max = String(maximumSupport);
  els.terrainSupport.disabled = !frame;
  if (terrainState.minimumSupport > maximumSupport) terrainState.minimumSupport = maximumSupport;
  els.terrainSupport.value = String(terrainState.minimumSupport);
  els.terrainSupportValue.textContent = formatTerrainCount(terrainState.minimumSupport);
  els.terrainRelationships.disabled = !frame?.relationship;
  if (!frame?.relationship) els.terrainRelationships.checked = false;
}

function captureTerrainResultSets(resultSets) {
  const nextData = buildTerrainData(resultSets);
  if (!nextData) return false;
  terrainData = nextData;
  terrainState.minimumSupport = 0;
  terrainState.relationships = true;
  terrainState.selectedType = null;
  terrainState.selectedId = null;
  els.terrainRelationships.checked = true;
  els.terrainSourceBadge.textContent = 'Query data';
  els.terrainSourceBadge.classList.add('live');
  els.terrainSourceText.textContent = `${formatTerrainCount(nextData.result_rows.history)} history rows · ${formatTerrainCount(nextData.result_rows.snapshot)} current rows`;
  renderTerrain();
  return true;
}

function clearTerrainData() {
  terrainData = null;
  terrainState.minimumSupport = 0;
  terrainState.relationships = true;
  terrainState.selectedType = null;
  terrainState.selectedId = null;
  els.terrainSourceBadge.textContent = 'Awaiting query';
  els.terrainSourceBadge.classList.remove('live');
  els.terrainSourceText.textContent = 'Run a Terrain incidence script to populate this workspace';
  els.terrainRelationships.checked = true;
  renderTerrain();
}

function initializeTerrain() {
  document.querySelectorAll('.terrain-scope').forEach(button => {
    button.addEventListener('click', () => {
      terrainState.scope = button.dataset.scope;
      terrainState.minimumSupport = 0;
      terrainState.selectedType = null;
      terrainState.selectedId = null;
      document.querySelectorAll('.terrain-scope').forEach(option => {
        const active = option === button;
        option.classList.toggle('active', active);
        option.setAttribute('aria-pressed', String(active));
      });
      renderTerrain();
    });
  });

  els.terrainSupport.addEventListener('input', () => {
    terrainState.minimumSupport = Number(els.terrainSupport.value);
    renderTerrain();
  });

  els.terrainRelationships.addEventListener('change', () => {
    terrainState.relationships = els.terrainRelationships.checked;
    if (!terrainState.relationships && ['relationship', 'allocation'].includes(terrainState.selectedType)) {
      terrainState.selectedType = null;
      terrainState.selectedId = null;
    }
    renderTerrain();
  });

  els.terrainDetail.addEventListener('click', event => {
    const button = event.target.closest('[data-terrain-query]');
    if (!button) return;
    const selected = terrainSelection();
    if (!selected) return;
    scriptEditor.textContent = terrainQuery(selected);
    updateHighlight();
    setWorkspaceMode('query');
    scriptEditor.focus();
    setStatus(`Query prepared from terrain ${terrainState.selectedType}`);
  });

  renderTerrain();
}

function renderTerrainStats() {
  const frame = terrainFrame();
  const stats = frame?.stats || {};
  const values = [
    ['Things', stats.things], ['Roles', stats.roles],
    ['Appearance sets', stats.appearance_sets], ['Posits', stats.posits]
  ];
  els.terrainStats.innerHTML = values.map(([label, value]) => `
    <div class="terrain-stat">
      <span>${escapeHtml(label)}</span>
      <strong>${value === undefined ? '—' : formatTerrainCount(value)}</strong>
    </div>
  `).join('') + `
    <div class="terrain-stat terrain-scope-stat">
      <span>Scope</span>
      <strong>${escapeHtml(frame?.label || 'No terrain query loaded')}</strong>
    </div>
  `;
}

function terrainRoundedPath(x, y, width, height, radius) {
  const right = x + width;
  const bottom = y + height;
  return `M${x + radius} ${y} H${right - radius} Q${right} ${y} ${right} ${y + radius} V${bottom - radius} Q${right} ${bottom} ${right - radius} ${bottom} H${x + radius} Q${x} ${bottom} ${x} ${bottom - radius} V${y + radius} Q${x} ${y} ${x + radius} ${y} Z`;
}

function terrainRoleLines(name) {
  const words = name.split(/\s+/).filter(Boolean);
  if (words.length <= 3) return words;
  return [words.slice(0, 2).join(' '), words.slice(2).join(' ')];
}

function terrainLayout(frame, visibleIsopleths, visibleAllocations) {
  const roles = Object.fromEntries(frame.projection.roles.map((role, index) => [role.id, {
    position: TERRAIN_ROLE_POSITIONS[index], lines: terrainRoleLines(role.name)
  }]));
  const isopleths = {};
  visibleIsopleths.forEach((isopleth, index) => {
    const points = isopleth.included_roles.map(roleId => roles[roleId]?.position).filter(Boolean);
    const xs = points.map(point => point[0]);
    const ys = points.map(point => point[1]);
    const padding = 34 + isopleth.included_roles.length * 9;
    const minX = Math.max(15, Math.min(...xs) - padding);
    const maxX = Math.min(748, Math.max(...xs) + padding);
    const minY = Math.max(15, Math.min(...ys) - padding);
    const maxY = Math.min(472, Math.max(...ys) + padding);
    const width = Math.max(108, maxX - minX);
    const height = Math.max(82, maxY - minY);
    const x = Math.max(15, Math.min(minX, 748 - width));
    const y = Math.max(15, Math.min(minY, 472 - height));
    isopleths[isopleth.id] = {
      path: terrainRoundedPath(x, y, width, height, Math.min(46, height / 2)),
      label_fraction: [0.16, 0.34, 0.58, 0.82][index % 4],
      tone: TERRAIN_TONES[index % TERRAIN_TONES.length]
    };
  });

  const panel = { x: 790, y: 110, width: 135, height: Math.max(170, 105 + visibleAllocations.length * 46) };
  const allocations = {};
  visibleAllocations.forEach((allocation, index) => {
    const portY = panel.y + 88 + index * 46;
    allocations[allocation.id] = {
      anchor_fraction: [0.18, 0.4, 0.62, 0.84][index % 4],
      port: [panel.x, portY],
      label: [panel.x + panel.width / 2, portY]
    };
  });
  return {
    roles,
    isopleths,
    relationship: { panel, label: [panel.x + panel.width / 2, panel.y + 34], allocations }
  };
}

function renderTerrainEmpty() {
  els.terrainMap.innerHTML = `
    <title id="terrainMapTitle">Terrain is waiting for query results</title>
    <desc id="terrainMapDescription">Run the supplied Terrain Traqula fixture to build this map from returned result cells.</desc>
    <rect class="terrain-background" width="940" height="500"></rect>
    <g class="terrain-empty" transform="translate(470 225)">
      <text class="terrain-empty-title" y="0">Run the Terrain incidence fixture</text>
      <text class="terrain-empty-copy" y="30">Paste traqula/terrain.traqula into Query, run it, then return to Terrain.</text>
    </g>
  `;
  els.terrainDetail.innerHTML = `
    <span class="detail-kind">No query data</span>
    <h3>Terrain is derived from results</h3>
    <p class="detail-copy">Run both incidence searches in the supplied fixture. The browser will recognize their columns and build history and current frames from the actual cells.</p>
    <a class="terrain-fixture-link" href="traqula/terrain.traqula" target="_blank" rel="noopener">Open the paste-ready fixture</a>
  `;
}

function renderTerrain() {
  renderTerrainStats();
  syncTerrainControls();
  const frame = terrainFrame();
  if (!frame) {
    renderTerrainEmpty();
    return;
  }

  const visibleIsopleths = frame.isopleths.filter(isopleth => isopleth.support >= terrainState.minimumSupport);
  const visibleIsoplethIds = new Set(visibleIsopleths.map(isopleth => isopleth.id));
  const visibleAllocations = (frame.relationship?.allocations || [])
    .filter(allocation => visibleIsoplethIds.has(allocation.isopleth_id));
  const layout = terrainLayout(frame, visibleIsopleths, visibleAllocations);

  if (terrainState.selectedType === 'isopleth' && !visibleIsoplethIds.has(terrainState.selectedId)) {
    terrainState.selectedType = null;
    terrainState.selectedId = null;
  }
  if (terrainState.selectedType === 'allocation' && !visibleAllocations.some(allocation => allocation.id === terrainState.selectedId)) {
    terrainState.selectedType = null;
    terrainState.selectedId = null;
  }
  if (!terrainSelection() && visibleIsopleths.length) {
    terrainState.selectedType = 'isopleth';
    terrainState.selectedId = visibleIsopleths[0].id;
  }

  const isoplethMarkup = visibleIsopleths.map(isopleth => {
    const itemLayout = layout.isopleths[isopleth.id];
    const selected = terrainState.selectedType === 'isopleth' && terrainState.selectedId === isopleth.id;
    const roles = terrainRoleNames(isopleth.included_roles).join(', ');
    return `
      <g class="terrain-isopleth tone-${itemLayout.tone}${selected ? ' selected' : ''}" tabindex="0" role="button"
         aria-label="${formatTerrainCount(isopleth.support)} Things have ${escapeHtml(roles)}"
         data-terrain-type="isopleth" data-terrain-id="${isopleth.id}">
        <path d="${itemLayout.path}"></path>
        <g class="isopleth-label" data-path-fraction="${itemLayout.label_fraction}">
          <rect x="-30" y="-13" width="60" height="24" rx="12"></rect>
          <text>${formatTerrainCount(isopleth.support)}</text>
        </g>
      </g>
    `;
  }).join('');

  const roleMarkup = frame.projection.roles.map(role => {
    const itemLayout = layout.roles[role.id];
    const selected = terrainState.selectedType === 'role' && terrainState.selectedId === role.id;
    const lineHeight = 17;
    const start = -((itemLayout.lines.length - 1) * lineHeight) / 2;
    return `
      <g class="terrain-role${selected ? ' selected' : ''}" tabindex="0" role="button"
         aria-label="Role ${escapeHtml(role.name)}, ${formatTerrainCount(role.distinct_things)} distinct Things"
         transform="translate(${itemLayout.position[0]} ${itemLayout.position[1]})"
         data-terrain-type="role" data-terrain-id="${role.id}">
        <text>${itemLayout.lines.map((line, index) => `<tspan x="0" y="${start + index * lineHeight}">${escapeHtml(line)}</tspan>`).join('')}</text>
      </g>
    `;
  }).join('');

  const relationship = frame.relationship;
  const relationshipLayout = layout.relationship;
  const relationshipSelected = terrainState.selectedType === 'relationship';
  const relationshipMarkup = terrainState.relationships && relationship && visibleAllocations.length ? `
    <g class="terrain-allocations">
      <g class="relationship-panel">
        <rect class="relationship-panel-background"
          x="${relationshipLayout.panel.x}" y="${relationshipLayout.panel.y}"
          width="${relationshipLayout.panel.width}" height="${relationshipLayout.panel.height}" rx="9"></rect>
      </g>
      ${visibleAllocations.map(allocation => {
        const itemLayout = relationshipLayout.allocations[allocation.id];
        const selected = terrainState.selectedType === 'allocation' && terrainState.selectedId === allocation.id;
        return `
        <g class="terrain-allocation${selected ? ' selected' : ''}" tabindex="0" role="button"
           aria-label="${escapeHtml(allocation.role)} allocation, ${formatTerrainCount(allocation.distinct_things)} unique Things, ${formatTerrainCount(allocation.participations)} participations"
           data-terrain-type="allocation" data-terrain-id="${allocation.id}">
          <path class="allocation-halo" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${itemLayout.anchor_fraction}"></path>
          <path class="allocation-line" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${itemLayout.anchor_fraction}"></path>
          <circle class="allocation-anchor" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${itemLayout.anchor_fraction}" r="6"></circle>
          <circle class="allocation-port" cx="${itemLayout.port[0]}" cy="${itemLayout.port[1]}" r="5"></circle>
          <g class="allocation-label" transform="translate(${itemLayout.label[0]} ${itemLayout.label[1]})">
            <rect x="-58" y="-19" width="116" height="38" rx="5"></rect>
            <text class="allocation-role" x="-47" y="-3">${escapeHtml(allocation.role)}</text>
            <text class="allocation-count" x="47" y="12">${formatTerrainCount(allocation.distinct_things)} (${formatTerrainCount(allocation.participations)})</text>
          </g>
        </g>
      `;
      }).join('')}
      <g class="relation-label${relationshipSelected ? ' selected' : ''}" tabindex="0" role="button"
         aria-label="Relationship signature ${escapeHtml(relationship.roles.join(', '))}, ${formatTerrainCount(relationship.appearance_sets)} appearance sets"
         transform="translate(${relationshipLayout.label[0]} ${relationshipLayout.label[1]})"
         data-terrain-type="relationship" data-terrain-id="${relationship.id}">
        <text class="relationship-kicker" y="-9">Exact signature</text>
        <text class="relationship-name" y="9">{${escapeHtml(relationship.roles.join(', '))}}</text>
        <text class="relationship-summary" y="27">${formatTerrainCount(relationship.appearance_sets)} sets / ${formatTerrainCount(relationship.posits)} posits</text>
      </g>
    </g>
  ` : '';

  const projectionNote = frame.projection.complete
    ? `Complete projection over ${frame.projection.roles.length} attribute Roles.`
    : `Showing ${frame.projection.roles.length} of ${frame.projection.total_roles} attribute Roles.`;
  els.terrainMap.innerHTML = `
    <title id="terrainMapTitle">Query-derived role-isopleth visualization</title>
    <desc id="terrainMapDescription">Role labels are enclosed by support isopleths. Relationship allocation lines connect an exact appearance-set signature to projected identity profiles.</desc>
    <rect class="terrain-background" width="940" height="500"></rect>
    <text class="terrain-axis-note" x="22" y="488">${escapeHtml(projectionNote)} Hidden isopleths do not imply zero.</text>
    ${isoplethMarkup}
    ${relationshipMarkup}
    ${roleMarkup}
  `;

  positionTerrainGeometry(layout);
  els.terrainMap.querySelectorAll('[data-terrain-id]').forEach(item => {
    const select = () => selectTerrainItem(item.dataset.terrainType, item.dataset.terrainId);
    item.addEventListener('click', select);
    item.addEventListener('keydown', event => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault();
      select();
    });
  });
  renderTerrainDetail();
}

function positionTerrainGeometry(layout) {
  els.terrainMap.querySelectorAll('.terrain-isopleth').forEach(group => {
    const path = group.querySelector(':scope > path');
    const label = group.querySelector('.isopleth-label');
    const point = path.getPointAtLength(path.getTotalLength() * Number(label.dataset.pathFraction));
    label.setAttribute('transform', `translate(${point.x} ${point.y})`);
  });

  els.terrainMap.querySelectorAll('[data-target-isopleth]').forEach(element => {
    const target = els.terrainMap.querySelector(
      `[data-terrain-type="isopleth"][data-terrain-id="${element.dataset.targetIsopleth}"] > path`
    );
    if (!target) return;
    const point = target.getPointAtLength(target.getTotalLength() * Number(element.dataset.pathFraction));
    const allocation = element.closest('.terrain-allocation');
    const itemLayout = layout.relationship.allocations[allocation.dataset.terrainId];
    if (element.tagName === 'circle') {
      element.setAttribute('cx', point.x);
      element.setAttribute('cy', point.y);
      return;
    }
    const route = `M${point.x} ${point.y} C${Math.max(point.x + 34, itemLayout.port[0] - 72)} ${point.y} ${itemLayout.port[0] - 42} ${itemLayout.port[1]} ${itemLayout.port[0]} ${itemLayout.port[1]}`;
    element.setAttribute('d', route);
  });
}

function renderTerrainDetail() {
  const selected = terrainSelection();
  if (!selected) {
    els.terrainDetail.innerHTML = '<p class="terrain-empty-detail">No isopleth meets the support threshold.</p>';
    return;
  }

  if (terrainState.selectedType === 'relationship') {
    els.terrainDetail.innerHTML = `
      <span class="detail-kind">Exact relationship signature</span>
      <h3>{${escapeHtml(selected.roles.join(', '))}}</h3>
      <p class="detail-copy">Each matching multi-role appearance set connects Things. Parentheses on the map show total participations.</p>
      <dl class="detail-metrics">
        <div><dt>Appearance sets</dt><dd>${formatTerrainCount(selected.appearance_sets)}</dd></div>
        <div><dt>Recorded posits</dt><dd>${formatTerrainCount(selected.posits)}</dd></div>
      </dl>
      <div class="endpoint-list">
        ${selected.role_totals.map(endpoint => `
          <div>
            <strong>${escapeHtml(endpoint.role)}</strong>
            <span>${formatTerrainCount(endpoint.distinct_things)} distinct / ${formatTerrainCount(endpoint.participations)} participations</span>
          </div>
        `).join('')}
      </div>
      <button class="terrain-query-button" type="button" data-terrain-query>Prepare query</button>
    `;
    return;
  }

  if (terrainState.selectedType === 'allocation') {
    const profile = terrainProfile(selected.profile_id);
    els.terrainDetail.innerHTML = `
      <span class="detail-kind">Relationship allocation</span>
      <h3>${escapeHtml(selected.role)} in ${escapeHtml(terrainProfileLabel(profile))}</h3>
      <p class="detail-copy">An endpoint cohort grouped by its exact projected attribute profile.</p>
      <dl class="detail-metrics">
        <div><dt>Unique ${escapeHtml(selected.role)} Things</dt><dd>${formatTerrainCount(selected.distinct_things)}</dd></div>
        <div><dt>Participations</dt><dd>${formatTerrainCount(selected.participations)}</dd></div>
        <div><dt>Profile population</dt><dd>${formatTerrainCount(profile.things)}</dd></div>
      </dl>
      <div class="role-token-list">${terrainRoleNames(profile.present_roles).map(role => `<span>${escapeHtml(role)}</span>`).join('')}</div>
      <p class="profile-exclusions">Excludes in projection: ${escapeHtml(terrainRoleNames(profile.absent_roles).join(', '))}</p>
      <button class="terrain-query-button" type="button" data-terrain-query>Prepare relationship query</button>
    `;
    return;
  }

  if (terrainState.selectedType === 'role') {
    const containing = terrainFrame().isopleths.filter(isopleth => isopleth.included_roles.includes(selected.id));
    els.terrainDetail.innerHTML = `
      <span class="detail-kind">Role</span>
      <h3>${escapeHtml(selected.name)}</h3>
      <p class="detail-copy">A fixed point in the selected attribute-Role projection. Surrounding lines show supported Role combinations.</p>
      <dl class="detail-metrics">
        <div><dt>Distinct Things</dt><dd>${formatTerrainCount(selected.distinct_things)}</dd></div>
        <div><dt>Derived isopleths</dt><dd>${containing.length}</dd></div>
      </dl>
      <button class="terrain-query-button" type="button" data-terrain-query>Prepare role query</button>
    `;
    return;
  }

  const profiles = terrainProfilesForIsopleth(selected);
  const sharedExclusions = terrainSharedExclusions(profiles);
  els.terrainDetail.innerHTML = `
    <span class="detail-kind">Support isopleth</span>
    <h3>${formatTerrainCount(selected.support)} Things</h3>
    <p class="detail-copy">Every counted Thing appears in every Role enclosed by this line.</p>
    <dl class="detail-metrics">
      <div><dt>Distinct Things</dt><dd>${formatTerrainCount(selected.support)}</dd></div>
      <div><dt>Enclosed Roles</dt><dd>${selected.included_roles.length}</dd></div>
      <div><dt>Projected profiles</dt><dd>${profiles.length}</dd></div>
    </dl>
    <div class="role-token-list">
      ${terrainRoleNames(selected.included_roles).map(role => `<span>${escapeHtml(role)}</span>`).join('')}
    </div>
    ${profiles.map(profile => `<div class="profile-row"><strong>${escapeHtml(terrainProfileLabel(profile))}</strong><span>${formatTerrainCount(profile.things)}</span></div>`).join('')}
    ${sharedExclusions.length ? `<p class="profile-exclusions">All exclude in projection: ${escapeHtml(terrainRoleNames(sharedExclusions).join(', '))}</p>` : ''}
    <button class="terrain-query-button" type="button" data-terrain-query>Prepare isopleth query</button>
  `;
}

function terrainProfileLabel(profile) {
  return terrainRoleNames(profile?.present_roles || []).join(' + ') || 'no projected attribute Roles';
}

function terrainProfilesForIsopleth(isopleth) {
  return terrainFrame().profiles.filter(profile =>
    isopleth.included_roles.every(roleId => profile.present_roles.includes(roleId))
  );
}

function terrainSharedExclusions(profiles) {
  if (!profiles.length) return [];
  return profiles[0].absent_roles.filter(roleId =>
    profiles.every(profile => profile.absent_roles.includes(roleId))
  );
}

function terrainRoleToken(role) {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(role) ? role : `\`${role.replaceAll('`', '``')}\``;
}

function terrainQuery(selected) {
  const asOf = terrainState.scope === 'snapshot' ? ' as of @NOW' : '';
  if (['relationship', 'allocation'].includes(terrainState.selectedType)) {
    const relationship = terrainFrame().relationship;
    const appearances = relationship.roles.map((role, index) => `(?member_${index + 1}, ${terrainRoleToken(role)})`).join(', ');
    const variables = relationship.roles.map((_, index) => `?member_${index + 1}`).join(', ');
    return `search [{${appearances}}, ?value, ?time]${asOf}\nreturn ${variables}, ?value, ?time;`;
  }
  if (terrainState.selectedType === 'role') {
    return `search [{(?thing, ${terrainRoleToken(selected.name)}), ...}, *, *]${asOf}\nreturn distinct ?thing;`;
  }
  const patterns = terrainRoleNames(selected.included_roles)
    .map(role => `[{(?thing, ${terrainRoleToken(role)}), ...}, *, *]${asOf}`);
  return `search ${patterns.join(',\n       ')}\nreturn distinct ?thing;`;
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    TERRAIN_REQUIRED_COLUMNS,
    buildTerrainData,
    buildTerrainFrame,
    isTerrainIncidenceResultSet,
    normalizeTerrainRows,
    terrainCellText
  };
}

if (typeof document !== 'undefined') {
  document.addEventListener('DOMContentLoaded', initializeTerrain);
}
