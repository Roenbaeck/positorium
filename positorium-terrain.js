const TERRAIN_MOCK_DATA = {
  schema_version: 2,
  source: 'mock',
  database: {
    things: 12480,
    roles: 10,
    appearance_sets: 9730,
    posits: 38205
  },
  frames: {
    history: {
      label: 'All recorded history',
      projection: {
        complete: true,
        roles: [
          { id: 'name', name: 'name', distinct_things: 5800 },
          { id: 'hair', name: 'hair color', distinct_things: 5800 },
          { id: 'height', name: 'height', distinct_things: 5000 },
          { id: 'ssn', name: 'social security number', distinct_things: 5000 },
          { id: 'beard', name: 'beard color', distinct_things: 1450 },
          { id: 'rfid', name: 'RFID', distinct_things: 750 }
        ]
      },
      profiles: [
        {
          id: 'name-hair-only',
          present_roles: ['name', 'hair'],
          absent_roles: ['height', 'ssn', 'beard', 'rfid'],
          things: 50
        },
        {
          id: 'height-ssn-only',
          present_roles: ['name', 'hair', 'height', 'ssn'],
          absent_roles: ['beard', 'rfid'],
          things: 3550
        },
        {
          id: 'beard-profile',
          present_roles: ['name', 'hair', 'height', 'ssn', 'beard'],
          absent_roles: ['rfid'],
          things: 1450
        },
        {
          id: 'rfid-profile',
          present_roles: ['name', 'hair', 'rfid'],
          absent_roles: ['height', 'ssn', 'beard'],
          things: 750
        }
      ],
      isopleths: [
        { id: 'name-hair', included_roles: ['name', 'hair'], support: 5800 },
        { id: 'height-ssn', included_roles: ['name', 'hair', 'height', 'ssn'], support: 5000 },
        { id: 'beard', included_roles: ['name', 'hair', 'height', 'ssn', 'beard'], support: 1450 },
        { id: 'rfid', included_roles: ['name', 'hair', 'rfid'], support: 750 }
      ],
      relationship: {
        id: 'owner-pet',
        roles: ['owner', 'pet'],
        appearance_sets: 600,
        posits: 720,
        role_totals: [
          { role: 'owner', distinct_things: 500, participations: 600 },
          { role: 'pet', distinct_things: 600, participations: 600 }
        ],
        allocations: [
          { id: 'owner-beard', role: 'owner', profile_id: 'beard-profile', isopleth_id: 'beard', distinct_things: 100, participations: 170 },
          { id: 'owner-height', role: 'owner', profile_id: 'height-ssn-only', isopleth_id: 'height-ssn', distinct_things: 400, participations: 430 },
          { id: 'pet-rfid', role: 'pet', profile_id: 'rfid-profile', isopleth_id: 'rfid', distinct_things: 600, participations: 600 }
        ]
      }
    },
    snapshot: {
      label: 'Maximal values as of now',
      projection: {
        complete: true,
        roles: [
          { id: 'name', name: 'name', distinct_things: 5700 },
          { id: 'hair', name: 'hair color', distinct_things: 5700 },
          { id: 'height', name: 'height', distinct_things: 4910 },
          { id: 'ssn', name: 'social security number', distinct_things: 4910 },
          { id: 'beard', name: 'beard color', distinct_things: 1400 },
          { id: 'rfid', name: 'RFID', distinct_things: 735 }
        ]
      },
      profiles: [
        {
          id: 'name-hair-only', present_roles: ['name', 'hair'], absent_roles: ['height', 'ssn', 'beard', 'rfid'], things: 55
        },
        {
          id: 'height-ssn-only', present_roles: ['name', 'hair', 'height', 'ssn'], absent_roles: ['beard', 'rfid'], things: 3510
        },
        {
          id: 'beard-profile', present_roles: ['name', 'hair', 'height', 'ssn', 'beard'], absent_roles: ['rfid'], things: 1400
        },
        {
          id: 'rfid-profile', present_roles: ['name', 'hair', 'rfid'], absent_roles: ['height', 'ssn', 'beard'], things: 735
        }
      ],
      isopleths: [
        { id: 'name-hair', included_roles: ['name', 'hair'], support: 5700 },
        { id: 'height-ssn', included_roles: ['name', 'hair', 'height', 'ssn'], support: 4910 },
        { id: 'beard', included_roles: ['name', 'hair', 'height', 'ssn', 'beard'], support: 1400 },
        { id: 'rfid', included_roles: ['name', 'hair', 'rfid'], support: 735 }
      ],
      relationship: {
        id: 'owner-pet',
        roles: ['owner', 'pet'],
        appearance_sets: 584,
        posits: 584,
        role_totals: [
          { role: 'owner', distinct_things: 492, participations: 584 },
          { role: 'pet', distinct_things: 584, participations: 584 }
        ],
        allocations: [
          { id: 'owner-beard', role: 'owner', profile_id: 'beard-profile', isopleth_id: 'beard', distinct_things: 96, participations: 164 },
          { id: 'owner-height', role: 'owner', profile_id: 'height-ssn-only', isopleth_id: 'height-ssn', distinct_things: 396, participations: 420 },
          { id: 'pet-rfid', role: 'pet', profile_id: 'rfid-profile', isopleth_id: 'rfid', distinct_things: 584, participations: 584 }
        ]
      }
    }
  }
};

const TERRAIN_MOCK_LAYOUT = {
  roles: {
    name: { position: [356, 155], lines: ['name'] },
    hair: { position: [356, 229], lines: ['hair', 'color'] },
    height: { position: [205, 222], lines: ['height'] },
    ssn: { position: [205, 302], lines: ['social', 'security', 'number'] },
    beard: { position: [85, 85], lines: ['beard', 'color'] },
    rfid: { position: [655, 270], lines: ['RFID'] }
  },
  isopleths: {
    'name-hair': {
      path: 'M286 105 C320 76 397 78 428 114 C457 148 457 222 429 258 C398 297 321 296 286 260 C252 225 251 143 286 105 Z',
      label_fraction: 0.6,
      tone: 'sea'
    },
    'height-ssn': {
      path: 'M140 85 C216 44 390 45 470 94 C528 130 537 250 483 323 C418 405 215 400 137 337 C76 288 78 136 140 85 Z',
      label_fraction: 0.9,
      tone: 'leaf'
    },
    beard: {
      path: 'M60 48 C175 -10 400 -7 522 58 C611 108 619 291 535 382 C440 482 166 469 57 378 C5 314 3 112 60 48 Z',
      label_fraction: 0.9,
      tone: 'earth'
    },
    rfid: {
      path: 'M316 68 C415 14 610 40 690 145 C752 238 718 380 623 424 C515 474 365 403 290 306 C218 210 228 108 316 68 Z',
      label_fraction: 0.48,
      tone: 'berry'
    }
  },
  relationship: {
    panel: { x: 790, y: 145, width: 135, height: 220 },
    label: [857.5, 174],
    allocations: {
      'owner-beard': { anchor_fraction: 0.4, port: [790, 230], label: [857.5, 230] },
      'owner-height': { anchor_fraction: 0.42, port: [790, 285], label: [857.5, 285], route_y: 185 },
      'pet-rfid': { anchor_fraction: 0.4, port: [790, 340], label: [857.5, 340] }
    }
  }
};

const terrainState = {
  scope: 'snapshot',
  minimumSupport: 0,
  relationships: true,
  selectedType: 'isopleth',
  selectedId: 'name-hair'
};

function formatTerrainCount(value) {
  return new Intl.NumberFormat().format(value);
}

function terrainFrame() {
  return TERRAIN_MOCK_DATA.frames[terrainState.scope];
}

function terrainSelection() {
  const frame = terrainFrame();
  if (terrainState.selectedType === 'relationship') return frame.relationship;
  if (terrainState.selectedType === 'allocation') {
    return frame.relationship.allocations.find(allocation => allocation.id === terrainState.selectedId);
  }
  if (terrainState.selectedType === 'role') {
    return frame.projection.roles.find(role => role.id === terrainState.selectedId);
  }
  return frame.isopleths.find(isopleth => isopleth.id === terrainState.selectedId);
}

function selectTerrainItem(type, id) {
  terrainState.selectedType = type;
  terrainState.selectedId = id;
  renderTerrain();
}

function initializeTerrain() {
  document.querySelectorAll('.terrain-scope').forEach(button => {
    button.addEventListener('click', () => {
      terrainState.scope = button.dataset.scope;
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
    els.terrainSupportValue.textContent = formatTerrainCount(terrainState.minimumSupport);
    renderTerrain();
  });

  els.terrainRelationships.addEventListener('change', () => {
    terrainState.relationships = els.terrainRelationships.checked;
    if (!terrainState.relationships && ['relationship', 'allocation'].includes(terrainState.selectedType)) {
      terrainState.selectedType = 'isopleth';
      terrainState.selectedId = 'name-hair';
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
    setStatus(`Query prepared from mock ${terrainState.selectedType}`);
  });

  renderTerrain();
}

function renderTerrainStats() {
  const stats = TERRAIN_MOCK_DATA.database;
  const values = [
    ['Things', stats.things],
    ['Roles', stats.roles],
    ['Appearance sets', stats.appearance_sets],
    ['Posits', stats.posits]
  ];
  els.terrainStats.innerHTML = values.map(([label, value]) => `
    <div class="terrain-stat">
      <span>${escapeHtml(label)}</span>
      <strong>${formatTerrainCount(value)}</strong>
    </div>
  `).join('') + `
    <div class="terrain-stat terrain-scope-stat">
      <span>Scope</span>
      <strong>${escapeHtml(terrainFrame().label)}</strong>
    </div>
  `;
}

function renderTerrain() {
  renderTerrainStats();
  const frame = terrainFrame();
  const visibleIsopleths = frame.isopleths.filter(isopleth => isopleth.support >= terrainState.minimumSupport);
  const visibleIsoplethIds = new Set(visibleIsopleths.map(isopleth => isopleth.id));
  const visibleAllocations = frame.relationship.allocations.filter(allocation => visibleIsoplethIds.has(allocation.isopleth_id));
  if (terrainState.selectedType === 'isopleth' && !visibleIsopleths.some(isopleth => isopleth.id === terrainState.selectedId)) {
    terrainState.selectedId = visibleIsopleths[0]?.id || null;
  }
  if (terrainState.selectedType === 'allocation' && !visibleAllocations.some(allocation => allocation.id === terrainState.selectedId)) {
    terrainState.selectedType = visibleAllocations.length ? 'relationship' : 'isopleth';
    terrainState.selectedId = visibleAllocations.length ? frame.relationship.id : (visibleIsopleths[0]?.id || null);
  }

  const isoplethMarkup = visibleIsopleths.map(isopleth => {
    const layout = TERRAIN_MOCK_LAYOUT.isopleths[isopleth.id];
    const selected = terrainState.selectedType === 'isopleth' && terrainState.selectedId === isopleth.id;
    const roles = terrainRoleNames(isopleth.included_roles).join(', ');
    return `
      <g class="terrain-isopleth tone-${layout.tone}${selected ? ' selected' : ''}" tabindex="0" role="button"
         aria-label="${formatTerrainCount(isopleth.support)} Things have ${escapeHtml(roles)}"
         data-terrain-type="isopleth" data-terrain-id="${isopleth.id}">
        <path d="${layout.path}"></path>
        <g class="isopleth-label" data-path-fraction="${layout.label_fraction}">
          <rect x="-30" y="-13" width="60" height="24" rx="12"></rect>
          <text>${formatTerrainCount(isopleth.support)}</text>
        </g>
      </g>
    `;
  }).join('');

  const roleMarkup = frame.projection.roles.map(role => {
    const layout = TERRAIN_MOCK_LAYOUT.roles[role.id];
    const selected = terrainState.selectedType === 'role' && terrainState.selectedId === role.id;
    const lineHeight = 17;
    const start = -((layout.lines.length - 1) * lineHeight) / 2;
    return `
      <g class="terrain-role${selected ? ' selected' : ''}" tabindex="0" role="button"
         aria-label="Role ${escapeHtml(role.name)}, ${formatTerrainCount(role.distinct_things)} distinct Things"
         transform="translate(${layout.position[0]} ${layout.position[1]})"
         data-terrain-type="role" data-terrain-id="${role.id}">
        <text>${layout.lines.map((line, index) => `<tspan x="0" y="${start + index * lineHeight}">${escapeHtml(line)}</tspan>`).join('')}</text>
      </g>
    `;
  }).join('');

  const relationship = frame.relationship;
  const relationshipLayout = TERRAIN_MOCK_LAYOUT.relationship;
  const relationshipSelected = terrainState.selectedType === 'relationship';
  const relationshipMarkup = terrainState.relationships && visibleAllocations.length ? `
    <g class="terrain-allocations">
      <g class="relationship-panel">
        <rect class="relationship-panel-background"
          x="${relationshipLayout.panel.x}" y="${relationshipLayout.panel.y}"
          width="${relationshipLayout.panel.width}" height="${relationshipLayout.panel.height}" rx="9"></rect>
      </g>
      ${visibleAllocations.map(allocation => {
        const layout = relationshipLayout.allocations[allocation.id];
        const selected = terrainState.selectedType === 'allocation' && terrainState.selectedId === allocation.id;
        return `
        <g class="terrain-allocation${selected ? ' selected' : ''}" tabindex="0" role="button"
           aria-label="${escapeHtml(allocation.role)} allocation, ${formatTerrainCount(allocation.distinct_things)} unique Things, ${formatTerrainCount(allocation.participations)} participations"
           data-terrain-type="allocation" data-terrain-id="${allocation.id}">
          <path class="allocation-halo" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${layout.anchor_fraction}"></path>
          <path class="allocation-line" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${layout.anchor_fraction}"></path>
          <circle class="allocation-anchor" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${layout.anchor_fraction}" r="6"></circle>
          <circle class="allocation-port" cx="${layout.port[0]}" cy="${layout.port[1]}" r="5"></circle>
          <g class="allocation-label" transform="translate(${layout.label[0]} ${layout.label[1]})">
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

  els.terrainMap.innerHTML = `
    <title id="terrainMapTitle">Mock role-isopleth visualization</title>
    <desc id="terrainMapDescription">Role labels are enclosed by support isopleths. Relationship allocation lines connect an exact appearance-set signature to projected identity profiles.</desc>
    <rect class="terrain-background" width="940" height="500"></rect>
    <text class="terrain-axis-note" x="22" y="488">Complete projection over six Roles. Hidden isopleths do not imply zero.</text>
    ${isoplethMarkup}
    ${relationshipMarkup}
    ${roleMarkup}
  `;

  positionTerrainGeometry();

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

function positionTerrainGeometry() {
  const relationshipLayout = TERRAIN_MOCK_LAYOUT.relationship;
  els.terrainMap.querySelectorAll('.terrain-isopleth').forEach(group => {
    const path = group.querySelector(':scope > path');
    const label = group.querySelector('.isopleth-label');
    const fraction = Number(label.dataset.pathFraction);
    const point = path.getPointAtLength(path.getTotalLength() * fraction);
    label.setAttribute('transform', `translate(${point.x} ${point.y})`);
  });

  els.terrainMap.querySelectorAll('[data-target-isopleth]').forEach(element => {
    const target = els.terrainMap.querySelector(
      `[data-terrain-type="isopleth"][data-terrain-id="${element.dataset.targetIsopleth}"] > path`
    );
    const fraction = Number(element.dataset.pathFraction);
    const point = target.getPointAtLength(target.getTotalLength() * fraction);
    const allocation = element.closest('.terrain-allocation');
    const layout = relationshipLayout.allocations[allocation.dataset.terrainId];
    if (element.tagName === 'circle') {
      element.setAttribute('cx', point.x);
      element.setAttribute('cy', point.y);
    } else {
      const route = layout.route_y === undefined
        ? `M${point.x} ${point.y} C${Math.max(point.x + 34, layout.port[0] - 72)} ${point.y} ${layout.port[0] - 42} ${layout.port[1]} ${layout.port[0]} ${layout.port[1]}`
        : `M${point.x} ${point.y} C${point.x + 36} ${point.y} ${point.x + 54} ${layout.route_y} ${point.x + 92} ${layout.route_y} S${layout.port[0] - 38} ${layout.port[1]} ${layout.port[0]} ${layout.port[1]}`;
      element.setAttribute(
        'd',
        route
      );
    }
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
      <p class="detail-copy">Each matching multi-role appearance set connects different Things. Parentheses show total participations.</p>
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
      <p class="detail-copy">A disjoint endpoint cohort within the selected Role projection.</p>
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
      <p class="detail-copy">A fixed point in the selected Role projection. Surrounding lines show supported Role combinations.</p>
      <dl class="detail-metrics">
        <div><dt>Distinct Things</dt><dd>${formatTerrainCount(selected.distinct_things)}</dd></div>
        <div><dt>Displayed isopleths</dt><dd>${containing.length}</dd></div>
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

function terrainRole(roleId) {
  return terrainFrame().projection.roles.find(role => role.id === roleId);
}

function terrainRoleNames(roleIds) {
  return roleIds.map(roleId => terrainRole(roleId)?.name || roleId);
}

function terrainProfile(profileId) {
  return terrainFrame().profiles.find(profile => profile.id === profileId);
}

function terrainProfileLabel(profile) {
  return terrainRoleNames(profile.present_roles).join(' + ');
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
  const patterns = terrainRoleNames(selected.included_roles).map(role => `[{(?thing, ${terrainRoleToken(role)}), ...}, *, *]${asOf}`);
  return `search ${patterns.join(',\n       ')}\nreturn distinct ?thing;`;
}

document.addEventListener('DOMContentLoaded', initializeTerrain);
