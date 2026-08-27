const TERRAIN_VERSION = 1;
const TERRAIN_ROLE_POSITIONS = [
  [356, 155], [356, 229], [205, 222], [205, 302],
  [85, 85], [655, 270], [520, 95], [575, 390]
];
const TERRAIN_TONES = ['sea', 'leaf', 'earth', 'berry'];

let terrainData = null;
const terrainState = {
  scope: 'current',
  minimumSupport: 0,
  relationships: true,
  selectedType: null,
  selectedId: null,
  selectedSignatureId: null,
  status: 'empty',
  source: null,
  error: null
};

function assertTerrainReport(report) {
  if (!report || report.terrain_version !== TERRAIN_VERSION) {
    throw new Error(`Unsupported Terrain report version ${report?.terrain_version ?? 'missing'}`);
  }
  if (!report.projection || !report.relationship_catalog || !report.frames?.history || !report.frames?.current) {
    throw new Error('Incomplete Terrain report');
  }
  return report;
}

function adaptTerrainFrame(report, frame, label) {
  const supports = new Map(frame.role_supports.map(support => [support.role_id, support.distinct_things]));
  const roleNames = new Map([
    ...report.projection.roles.map(role => [role.id, role.name]),
    ...report.relationship_catalog.signatures.flatMap(signature =>
      signature.roles.map(role => [role.id, role.name])
    )
  ]);
  const signatures = new Map(report.relationship_catalog.signatures.map(signature => [signature.id, signature]));
  return {
    scope: frame.scope,
    label,
    stats: {
      things: frame.stats.endpoint_things,
      roles: frame.stats.roles,
      appearance_sets: frame.stats.appearance_sets,
      posits: frame.stats.posits,
      incidences: frame.stats.incidences
    },
    projection: {
      complete: report.projection.complete,
      total_roles: report.projection.total_attribute_roles,
      roles: report.projection.roles.map(role => ({
        ...role,
        distinct_things: supports.get(role.id) || 0
      }))
    },
    profiles: frame.profiles.map(profile => ({
      ...profile,
      present_roles: profile.present_role_ids,
      absent_roles: profile.absent_role_ids
    })),
    isopleths: frame.isopleths.map(isopleth => ({
      ...isopleth,
      included_roles: isopleth.included_role_ids
    })),
    relationships: frame.relationships.map(relationship => {
      const signature = signatures.get(relationship.signature_id);
      if (!signature) throw new Error(`Unknown Terrain relationship signature ${relationship.signature_id}`);
      return {
        ...relationship,
        id: relationship.signature_id,
        roles: signature.roles.map(role => role.name),
        role_totals: relationship.role_totals.map(total => ({
          ...total,
          role: roleNames.get(total.role_id) || total.role_id
        })),
        allocations: relationship.allocations.map(allocation => ({
          ...allocation,
          role: roleNames.get(allocation.role_id) || allocation.role_id
        }))
      };
    })
  };
}

function adaptTerrainReport(rawReport) {
  const report = assertTerrainReport(rawReport);
  return {
    ...report,
    source: 'database_snapshot',
    frames: {
      history: adaptTerrainFrame(report, report.frames.history, 'All recorded history'),
      current: adaptTerrainFrame(
        report,
        report.frames.current,
        `Maximal values as of ${report.resolved_as_of}`
      )
    }
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

function terrainRelationship() {
  const frame = terrainFrame();
  if (!frame || !terrainState.selectedSignatureId) return null;
  return frame.relationships.find(relationship => relationship.signature_id === terrainState.selectedSignatureId) || null;
}

function terrainSelection() {
  const frame = terrainFrame();
  if (!frame) return null;
  const relationship = terrainRelationship();
  if (terrainState.selectedType === 'relationship') return relationship;
  if (terrainState.selectedType === 'allocation') {
    return relationship?.allocations.find(allocation => allocation.id === terrainState.selectedId) || null;
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
  const relationship = terrainRelationship();
  const maximumSupport = frame ? Math.max(1, ...frame.isopleths.map(isopleth => isopleth.support)) : 1;
  els.terrainSupport.max = String(maximumSupport);
  els.terrainSupport.disabled = !frame;
  if (terrainState.minimumSupport > maximumSupport) terrainState.minimumSupport = maximumSupport;
  els.terrainSupport.value = String(terrainState.minimumSupport);
  els.terrainSupportValue.textContent = formatTerrainCount(terrainState.minimumSupport);
  els.terrainRelationships.disabled = !relationship;
  if (!relationship) els.terrainRelationships.checked = false;
  const signatures = terrainData?.relationship_catalog?.signatures || [];
  els.terrainSignature.innerHTML = signatures.map(signature =>
    `<option value="${escapeHtml(signature.id)}">{${escapeHtml(signature.roles.map(role => role.name).join(', '))}}</option>`
  ).join('');
  els.terrainSignature.value = terrainState.selectedSignatureId || '';
  els.terrainSignature.disabled = signatures.length <= 1;
}

function updateTerrainSource() {
  const browser = terrainState.source === 'wasm';
  const source = browser ? 'Browser database snapshot' : 'Database snapshot';
  els.terrainSourceBadge.classList.remove('live', 'loading', 'stale', 'error');
  if (terrainState.status === 'loading') {
    els.terrainSourceBadge.textContent = 'Loading';
    els.terrainSourceBadge.classList.add('loading');
    els.terrainSourceText.textContent = `Refreshing ${source.toLowerCase()}…`;
  } else if (terrainState.status === 'ready') {
    els.terrainSourceBadge.textContent = source;
    els.terrainSourceBadge.classList.add('live');
    els.terrainSourceText.textContent = `${formatTerrainCount(terrainData.database.posits)} recorded posits · current cutoff ${terrainData.resolved_as_of}`;
  } else if (terrainState.status === 'stale') {
    els.terrainSourceBadge.textContent = 'Stale';
    els.terrainSourceBadge.classList.add('stale');
    els.terrainSourceText.textContent = terrainState.error || 'Database activity may have changed this snapshot; refresh Terrain.';
  } else if (terrainState.status === 'error') {
    els.terrainSourceBadge.textContent = 'Error';
    els.terrainSourceBadge.classList.add('error');
    els.terrainSourceText.textContent = terrainState.error || 'Terrain refresh failed.';
  } else {
    els.terrainSourceBadge.textContent = 'Not loaded';
    els.terrainSourceText.textContent = 'Open or refresh Terrain to capture the database.';
  }
  els.terrainRefresh.disabled = terrainState.status === 'loading';
}

function captureTerrainReport(report, source) {
  terrainData = adaptTerrainReport(report);
  terrainState.source = source;
  terrainState.status = 'ready';
  terrainState.error = null;
  terrainState.minimumSupport = 0;
  terrainState.relationships = true;
  terrainState.selectedType = null;
  terrainState.selectedId = null;
  terrainState.selectedSignatureId = terrainData.relationship_catalog.default_signature_id;
  els.terrainRelationships.checked = true;
  updateTerrainSource();
  renderTerrain();
}

function clearTerrainData() {
  terrainData = null;
  terrainState.status = 'empty';
  terrainState.source = null;
  terrainState.error = null;
  terrainState.minimumSupport = 0;
  terrainState.relationships = true;
  terrainState.selectedType = null;
  terrainState.selectedId = null;
  terrainState.selectedSignatureId = null;
  els.terrainRelationships.checked = true;
  updateTerrainSource();
  renderTerrain();
}

function markTerrainStale() {
  if (!terrainData || terrainState.status === 'loading') return;
  terrainState.status = 'stale';
  terrainState.error = null;
  updateTerrainSource();
}

function terrainEndpoint(queryEndpoint) {
  if (!/\/v1\/query\/?$/.test(queryEndpoint)) {
    throw new Error('The configured endpoint must end in /v1/query to locate /v1/terrain');
  }
  return queryEndpoint.replace(/\/v1\/query\/?$/, '/v1/terrain');
}

async function ensureTerrainWasmEngine() {
  if (!wasmEngine) {
    const pkg = await loadWasmPackage();
    await pkg.default();
    wasmEngine = new pkg.WasmEngine();
  }
  if (typeof wasmEngine.terrain !== 'function') {
    const packageLabel = wasmPackageSource === 'published' ? 'published fallback' : 'workspace';
    throw new Error(`The ${packageLabel} WASM package does not implement Terrain contract 1. Rebuild or update the package.`);
  }
  return wasmEngine;
}

async function refreshTerrain() {
  if (terrainState.status === 'loading') return;
  const previous = terrainData;
  const source = els.wasmMode.checked ? 'wasm' : 'http';
  terrainState.status = 'loading';
  terrainState.source = source;
  terrainState.error = null;
  updateTerrainSource();
  renderTerrain();
  const timeoutMs = Math.max(1, parseInt(els.timeout.value || '5000', 10));
  const options = {
    terrain_version: TERRAIN_VERSION,
    timeout_ms: timeoutMs,
    projected_role_limit: 8,
    max_relationship_signatures: 16
  };
  try {
    let report;
    if (source === 'wasm') {
      const engine = await ensureTerrainWasmEngine();
      const response = engine.terrain(options);
      if (response.interface_version !== '1' || response.terrain_version !== TERRAIN_VERSION) {
        throw new Error(`Unsupported WASM Terrain contract ${response.terrain_version ?? 'missing'}`);
      }
      report = response.report;
    } else {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), timeoutMs + 1_000);
      let response;
      try {
        response = await fetch(terrainEndpoint(els.endpoint.value.trim()), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(options),
          signal: controller.signal
        });
      } finally {
        clearTimeout(timer);
      }
      const payload = await response.json();
      if (!response.ok || payload.status !== 'ok') {
        throw new Error(payload.error || `Terrain request failed with HTTP ${response.status}`);
      }
      if (payload.api_version !== 'v1' || payload.terrain_version !== TERRAIN_VERSION) {
        throw new Error(`Unsupported HTTP Terrain contract ${payload.terrain_version ?? 'missing'}`);
      }
      report = payload.report;
    }
    captureTerrainReport(report, source);
  } catch (error) {
    terrainData = previous;
    terrainState.status = previous ? 'stale' : 'error';
    terrainState.error = error.message || String(error);
    updateTerrainSource();
    renderTerrain();
  }
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

  els.terrainSignature.addEventListener('change', () => {
    terrainState.selectedSignatureId = els.terrainSignature.value || null;
    terrainState.selectedType = null;
    terrainState.selectedId = null;
    renderTerrain();
  });

  els.terrainRefresh.addEventListener('click', refreshTerrain);

  els.terrainDetail.addEventListener('click', event => {
    if (event.target.closest('[data-terrain-refresh]')) {
      refreshTerrain();
      return;
    }
    const allocation = event.target.closest('[data-terrain-allocation-id]');
    if (allocation) {
      selectTerrainItem('allocation', allocation.dataset.terrainAllocationId);
      return;
    }
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

  updateTerrainSource();
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
      <strong>${escapeHtml(frame?.label || 'No database snapshot loaded')}</strong>
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

function renderTerrainEmpty(state = terrainState.status) {
  const messages = {
    loading: ['Capturing database snapshot', 'Terrain is reading one coherent structural state.'],
    error: ['Terrain refresh failed', terrainState.error || 'The database snapshot could not be loaded.'],
    empty: ['Terrain is ready to load', 'Refresh to build an authoritative database snapshot.'],
    report_empty: ['Database snapshot is empty', 'Add Roles and Posits, then refresh Terrain to map their structure.']
  };
  const [title, copy] = messages[state] || messages.empty;
  els.terrainMap.innerHTML = `
    <title id="terrainMapTitle">${escapeHtml(title)}</title>
    <desc id="terrainMapDescription">${escapeHtml(copy)}</desc>
    <rect class="terrain-background" width="940" height="500"></rect>
    <g class="terrain-empty" transform="translate(470 225)">
      <text class="terrain-empty-title" y="0">${escapeHtml(title)}</text>
      <text class="terrain-empty-copy" y="30">${escapeHtml(copy)}</text>
    </g>
  `;
  els.terrainDetail.innerHTML = `
    <span class="detail-kind">${state === 'error' ? 'Snapshot error' : 'Database snapshot'}</span>
    <h3>${escapeHtml(title)}</h3>
    <p class="detail-copy">${escapeHtml(copy)}</p>
    <button class="terrain-query-button" type="button" data-terrain-refresh>Refresh Terrain</button>
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
  if (frame.stats.appearance_sets === 0) {
    renderTerrainEmpty('report_empty');
    return;
  }

  const visibleIsopleths = frame.isopleths.filter(isopleth => isopleth.support >= terrainState.minimumSupport);
  const visibleIsoplethIds = new Set(visibleIsopleths.map(isopleth => isopleth.id));
  const relationship = terrainRelationship();
  const visibleAllocations = (relationship?.allocations || [])
    .filter(allocation => visibleIsoplethIds.has(allocation.isopleth_id));
  const layout = terrainLayout(frame, visibleIsopleths, visibleAllocations);

  if (terrainState.selectedType === 'isopleth' && !visibleIsoplethIds.has(terrainState.selectedId)) {
    terrainState.selectedType = null;
    terrainState.selectedId = null;
  }
  if (terrainState.selectedType === 'allocation') {
    const allocation = relationship?.allocations.find(candidate => candidate.id === terrainState.selectedId);
    if (!allocation || (allocation.profile_mask !== 0 && !visibleAllocations.some(candidate => candidate.id === terrainState.selectedId))) {
      terrainState.selectedType = null;
      terrainState.selectedId = null;
    }
  }
  if (!terrainSelection() && visibleIsopleths.length) {
    terrainState.selectedType = 'isopleth';
    terrainState.selectedId = visibleIsopleths[0].id;
  } else if (!terrainSelection() && terrainState.relationships && relationship) {
    terrainState.selectedType = 'relationship';
    terrainState.selectedId = relationship.id;
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

  const relationshipLayout = layout.relationship;
  const relationshipSelected = terrainState.selectedType === 'relationship';
  const relationshipMarkup = terrainState.relationships && relationship ? `
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
    <title id="terrainMapTitle">Authoritative role-isopleth visualization</title>
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
    const maskZeroAllocations = selected.allocations.filter(allocation => allocation.profile_mask === 0);
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
      ${maskZeroAllocations.length ? `
        <p class="profile-exclusions">Relationship-only endpoint cohorts</p>
        <div class="endpoint-list">
          ${maskZeroAllocations.map(allocation => `
            <button type="button" data-terrain-allocation-id="${escapeHtml(allocation.id)}">
              <strong>${escapeHtml(allocation.role)}</strong>
              <span>${formatTerrainCount(allocation.distinct_things)} distinct / ${formatTerrainCount(allocation.participations)} participations · no projected attributes</span>
            </button>
          `).join('')}
        </div>
      ` : ''}
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
  const asOf = terrainState.scope === 'current' ? ` as of ${terrainData.resolved_as_of}` : '';
  if (['relationship', 'allocation'].includes(terrainState.selectedType)) {
    const relationship = terrainRelationship();
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
    TERRAIN_VERSION,
    adaptTerrainReport,
    assertTerrainReport,
    terrainEndpoint
  };
}

if (typeof document !== 'undefined') {
  document.addEventListener('DOMContentLoaded', initializeTerrain);
}
