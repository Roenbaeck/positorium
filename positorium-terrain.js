const TERRAIN_VERSION = 1;
const TERRAIN_VIEWBOX = { width: 940, height: 500 };
const TERRAIN_PLOT = { left: 54, right: 886, top: 36, bottom: 342 };
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

function terrainTopologyIsopleths() {
  const topology = new Map();
  ['history', 'current'].forEach(scope => {
    (terrainData?.frames?.[scope]?.isopleths || []).forEach(isopleth => {
      const existing = topology.get(isopleth.id);
      if (!existing) {
        topology.set(isopleth.id, { ...isopleth });
      } else {
        existing.support = Math.max(existing.support, isopleth.support);
      }
    });
  });
  return [...topology.values()];
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

function terrainRoleLines(name) {
  const words = name.split(/\s+/).filter(Boolean);
  if (words.length <= 3) return words;
  return [words.slice(0, 2).join(' '), words.slice(2).join(' ')];
}

function terrainCompareRoleIds(left, right) {
  const leftNumber = Number(left);
  const rightNumber = Number(right);
  if (Number.isFinite(leftNumber) && Number.isFinite(rightNumber) && leftNumber !== rightNumber) {
    return leftNumber - rightNumber;
  }
  return String(left).localeCompare(String(right));
}

function terrainIsSubset(subset, superset) {
  return subset.every(value => superset.includes(value));
}

function terrainHierarchy(visibleIsopleths) {
  const nodes = visibleIsopleths.map((isopleth, index) => ({
    isopleth,
    index,
    parent: null,
    children: [],
    depth: 0,
    direction: Math.PI
  }));
  const ordered = [...nodes].sort((left, right) =>
    left.isopleth.included_roles.length - right.isopleth.included_roles.length ||
    right.isopleth.support - left.isopleth.support ||
    left.isopleth.id.localeCompare(right.isopleth.id)
  );

  ordered.forEach(node => {
    const candidates = ordered.filter(candidate =>
      candidate !== node &&
      candidate.isopleth.included_roles.length < node.isopleth.included_roles.length &&
      terrainIsSubset(candidate.isopleth.included_roles, node.isopleth.included_roles)
    );
    node.parent = candidates.sort((left, right) =>
      right.isopleth.included_roles.length - left.isopleth.included_roles.length ||
      right.isopleth.support - left.isopleth.support ||
      left.isopleth.id.localeCompare(right.isopleth.id)
    )[0] || null;
    if (node.parent) {
      node.parent.children.push(node);
      node.depth = node.parent.depth + 1;
    }
  });

  nodes.forEach(node => node.children.sort((left, right) =>
    right.isopleth.support - left.isopleth.support ||
    right.isopleth.included_roles.length - left.isopleth.included_roles.length ||
    left.isopleth.id.localeCompare(right.isopleth.id)
  ));
  return { nodes, roots: nodes.filter(node => !node.parent), ordered };
}

function terrainChildAngles(node) {
  const count = node.children.length;
  if (!count) return [];
  if (!node.parent) {
    if (count === 1) return [Math.PI];
    if (count === 2) return [Math.PI, 0];
    if (count === 3) return [Math.PI, -Math.PI / 3, Math.PI / 3];
    if (count === 4) return [Math.PI, -Math.PI / 2, 0, Math.PI / 2];
    return Array.from({ length: count }, (_, index) => Math.PI + index * Math.PI * 2 / count);
  }
  if (count === 1) return [node.direction + 0.42];
  const spread = Math.min(1.15, 0.62 * (count - 1));
  return Array.from({ length: count }, (_, index) =>
    node.direction - spread / 2 + (spread * index) / (count - 1)
  );
}

function terrainPlaceRoleGroup(roleIds, anchor, direction, roles) {
  const ordered = [...roleIds].sort(terrainCompareRoleIds);
  if (ordered.length === 1) {
    roles[ordered[0]].position = [...anchor];
    return;
  }
  if (ordered.length === 2) {
    const gap = 54;
    const normal = [Math.sin(direction), -Math.cos(direction)];
    ordered.forEach((roleId, index) => {
      const offset = (index - 0.5) * gap;
      roles[roleId].position = [anchor[0] + normal[0] * offset, anchor[1] + normal[1] * offset];
    });
    return;
  }
  const radius = 34 + ordered.length * 3;
  ordered.forEach((roleId, index) => {
    const angle = -Math.PI / 2 + index * Math.PI * 2 / ordered.length;
    roles[roleId].position = [anchor[0] + Math.cos(angle) * radius, anchor[1] + Math.sin(angle) * radius];
  });
}

function terrainFitRolePositions(roles) {
  const items = Object.values(roles).filter(role => role.position);
  if (!items.length) return;
  const xs = items.map(role => role.position[0]);
  const ys = items.map(role => role.position[1]);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const minY = Math.min(...ys);
  const maxY = Math.max(...ys);
  const availableWidth = TERRAIN_PLOT.right - TERRAIN_PLOT.left - 90;
  const availableHeight = TERRAIN_PLOT.bottom - TERRAIN_PLOT.top - 70;
  const scale = Math.min(
    1,
    availableWidth / Math.max(1, maxX - minX),
    availableHeight / Math.max(1, maxY - minY)
  );
  const sourceCenter = [(minX + maxX) / 2, (minY + maxY) / 2];
  const targetCenter = [
    (TERRAIN_PLOT.left + TERRAIN_PLOT.right) / 2,
    (TERRAIN_PLOT.top + TERRAIN_PLOT.bottom) / 2 - 8
  ];
  items.forEach(role => {
    role.position = [
      targetCenter[0] + (role.position[0] - sourceCenter[0]) * scale,
      targetCenter[1] + (role.position[1] - sourceCenter[1]) * scale
    ];
  });
}

function terrainConvexHull(points) {
  const ordered = [...points].sort((left, right) => left[0] - right[0] || left[1] - right[1]);
  if (ordered.length <= 2) return ordered;
  const cross = (origin, left, right) =>
    (left[0] - origin[0]) * (right[1] - origin[1]) -
    (left[1] - origin[1]) * (right[0] - origin[0]);
  const lower = [];
  ordered.forEach(point => {
    while (lower.length >= 2 && cross(lower.at(-2), lower.at(-1), point) <= 0) lower.pop();
    lower.push(point);
  });
  const upper = [];
  [...ordered].reverse().forEach(point => {
    while (upper.length >= 2 && cross(upper.at(-2), upper.at(-1), point) <= 0) upper.pop();
    upper.push(point);
  });
  lower.pop();
  upper.pop();
  return lower.concat(upper);
}

function terrainSmoothHullPath(points) {
  if (!points.length) return '';
  const midpoint = (left, right) => [(left[0] + right[0]) / 2, (left[1] + right[1]) / 2];
  const first = midpoint(points.at(-1), points[0]);
  let path = `M${first[0].toFixed(1)} ${first[1].toFixed(1)}`;
  points.forEach((point, index) => {
    const next = midpoint(point, points[(index + 1) % points.length]);
    path += ` Q${point[0].toFixed(1)} ${point[1].toFixed(1)} ${next[0].toFixed(1)} ${next[1].toFixed(1)}`;
  });
  return `${path} Z`;
}

function terrainRoleExtent(role) {
  const longestLine = Math.max(...role.lines.map(line => line.length), 1);
  return {
    x: Math.max(27, longestLine * 4.3),
    y: Math.max(19, role.lines.length * 10)
  };
}

function terrainContour(node, roles) {
  const siblings = node.parent ? node.parent.children : [];
  const siblingIndex = Math.max(0, siblings.indexOf(node));
  const siblingBias = siblings.length > 1 ? (siblingIndex - (siblings.length - 1) / 2) * 8 : 0;
  const padding = 27 + node.depth * 12 + siblingBias;
  const perimeter = [];
  node.isopleth.included_roles.forEach(roleId => {
    const role = roles[roleId];
    if (!role?.position) return;
    const extent = terrainRoleExtent(role);
    const radiusX = extent.x + padding;
    const radiusY = extent.y + padding * 0.76;
    for (let sample = 0; sample < 16; sample++) {
      const angle = sample * Math.PI * 2 / 16;
      perimeter.push([
        Math.max(18, Math.min(TERRAIN_VIEWBOX.width - 18, role.position[0] + Math.cos(angle) * radiusX)),
        Math.max(18, Math.min(356, role.position[1] + Math.sin(angle) * radiusY))
      ]);
    }
  });
  const hull = terrainConvexHull(perimeter);
  const center = node.isopleth.included_roles.reduce((total, roleId) => {
    const position = roles[roleId]?.position;
    if (position) {
      total[0] += position[0];
      total[1] += position[1];
      total[2] += 1;
    }
    return total;
  }, [0, 0, 0]);
  return {
    path: terrainSmoothHullPath(hull),
    label_fraction: 0.14 + (node.index % 5) * 0.16,
    tone: TERRAIN_TONES[node.index % TERRAIN_TONES.length],
    depth: node.depth,
    parent_id: node.parent?.isopleth.id || null,
    center: [center[0] / Math.max(1, center[2]), center[1] / Math.max(1, center[2])]
  };
}

function terrainLayout(frame, visibleIsopleths, visibleAllocations) {
  const roles = Object.fromEntries(frame.projection.roles.map(role => [role.id, {
    position: null,
    lines: terrainRoleLines(role.name)
  }]));
  const hierarchy = terrainHierarchy(visibleIsopleths);
  const roleOwners = new Map();
  hierarchy.ordered.forEach(node => {
    node.isopleth.included_roles.forEach(roleId => {
      if (!roleOwners.has(roleId)) roleOwners.set(roleId, node);
    });
  });
  const rootGap = hierarchy.roots.length > 1 ? 300 / Math.max(1, hierarchy.roots.length - 1) : 0;
  hierarchy.roots.forEach((root, rootIndex) => {
    root.anchor = [
      TERRAIN_VIEWBOX.width / 2 + (rootIndex - (hierarchy.roots.length - 1) / 2) * rootGap,
      167 + (rootIndex % 2) * 28
    ];
    root.direction = Math.PI;
  });

  const placeNode = node => {
    const introducedRoles = node.isopleth.included_roles.filter(roleId => roleOwners.get(roleId) === node);
    terrainPlaceRoleGroup(introducedRoles, node.anchor, node.parent ? node.direction : Math.PI, roles);
    const childAngles = terrainChildAngles(node);
    node.children.forEach((child, index) => {
      child.direction = childAngles[index];
      const introducedCount = child.isopleth.included_roles.filter(roleId => roleOwners.get(roleId) === child).length;
      const distance = 142 + Math.max(0, introducedCount - 1) * 16;
      child.anchor = [
        node.anchor[0] + Math.cos(child.direction) * distance,
        node.anchor[1] + Math.sin(child.direction) * distance * 0.62
      ];
      placeNode(child);
    });
  };
  hierarchy.roots.forEach(placeNode);

  const unplaced = frame.projection.roles.filter(role => !roles[role.id].position);
  unplaced.forEach((role, index) => {
    const columns = Math.min(5, Math.max(1, unplaced.length));
    roles[role.id].position = [
      TERRAIN_VIEWBOX.width / 2 + (index % columns - (columns - 1) / 2) * 110,
      305 + Math.floor(index / columns) * 44
    ];
  });
  terrainFitRolePositions(roles);

  const isopleths = {};
  hierarchy.nodes.forEach(node => {
    isopleths[node.isopleth.id] = terrainContour(node, roles);
  });

  const allocationOrder = [...visibleAllocations].sort((left, right) => {
    const leftX = isopleths[left.isopleth_id]?.center[0] ?? TERRAIN_VIEWBOX.width / 2;
    const rightX = isopleths[right.isopleth_id]?.center[0] ?? TERRAIN_VIEWBOX.width / 2;
    return leftX - rightX || left.id.localeCompare(right.id);
  });
  const portGap = Math.min(124, 330 / Math.max(1, allocationOrder.length - 1));
  const allocations = {};
  allocationOrder.forEach((allocation, index) => {
    const portX = TERRAIN_VIEWBOX.width / 2 + (index - (allocationOrder.length - 1) / 2) * portGap;
    allocations[allocation.id] = {
      anchor_fraction: 0.5,
      port: [portX, 392],
      label: [portX, 407],
      branch_end: [TERRAIN_VIEWBOX.width / 2 + (index - (allocationOrder.length - 1) / 2) * 34, 440]
    };
  });
  return {
    roles,
    isopleths,
    relationship: { label: [TERRAIN_VIEWBOX.width / 2, 457], allocations }
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
  // Keep Role positions stable across scope switches and support filtering.
  const layout = terrainLayout(frame, terrainTopologyIsopleths(), visibleAllocations);

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
      ${visibleAllocations.map(allocation => {
        const itemLayout = relationshipLayout.allocations[allocation.id];
        const selected = terrainState.selectedType === 'allocation' && terrainState.selectedId === allocation.id;
        return `
        <g class="terrain-allocation${selected ? ' selected' : ''}" tabindex="0" role="button"
           aria-label="${escapeHtml(allocation.role)} allocation, ${formatTerrainCount(allocation.distinct_things)} unique Things, ${formatTerrainCount(allocation.participations)} participations"
           data-terrain-type="allocation" data-terrain-id="${allocation.id}">
          <path class="allocation-halo" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${itemLayout.anchor_fraction}"></path>
          <path class="allocation-line" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${itemLayout.anchor_fraction}"></path>
          <path class="allocation-branch" d="M${itemLayout.label[0]} ${itemLayout.label[1] + 16} L${itemLayout.branch_end[0]} ${itemLayout.branch_end[1]}"></path>
          <circle class="allocation-anchor" data-target-isopleth="${allocation.isopleth_id}" data-path-fraction="${itemLayout.anchor_fraction}" r="6"></circle>
          <circle class="allocation-port" cx="${itemLayout.port[0]}" cy="${itemLayout.port[1]}" r="5"></circle>
          <g class="allocation-label" transform="translate(${itemLayout.label[0]} ${itemLayout.label[1]})">
            <rect x="-54" y="-15" width="108" height="30" rx="15"></rect>
            <text class="allocation-role" x="-42" y="4">${escapeHtml(allocation.role)}</text>
            <text class="allocation-count" x="42" y="4">${formatTerrainCount(allocation.distinct_things)} (${formatTerrainCount(allocation.participations)})</text>
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
  const rolePositions = Object.values(layout.roles).map(role => role.position);
  const labelPositions = [];
  els.terrainMap.querySelectorAll('.terrain-isopleth').forEach(group => {
    const path = group.querySelector(':scope > path');
    const label = group.querySelector('.isopleth-label');
    const length = path.getTotalLength();
    const candidates = Array.from({ length: 32 }, (_, index) => {
      const fraction = (index + 0.5) / 32;
      const point = path.getPointAtLength(length * fraction);
      const roleClearance = Math.min(...rolePositions.map(position =>
        Math.hypot(point.x - position[0], point.y - position[1])
      ));
      const labelClearance = labelPositions.length
        ? Math.min(...labelPositions.map(position => Math.hypot(point.x - position[0], point.y - position[1])))
        : 120;
      const bottomPenalty = Math.max(0, point.y - 320) * 4;
      const edgePenalty = Math.max(0, 44 - point.x) * 4 + Math.max(0, point.x - 896) * 4;
      return { fraction, point, score: roleClearance + Math.min(90, labelClearance) - bottomPenalty - edgePenalty };
    });
    const preferredFraction = Number(label.dataset.pathFraction);
    candidates.forEach(candidate => {
      candidate.score -= Math.abs(candidate.fraction - preferredFraction) * 8;
    });
    const point = candidates.sort((left, right) => right.score - left.score)[0].point;
    label.setAttribute('transform', `translate(${point.x} ${point.y})`);
    labelPositions.push([point.x, point.y]);
  });

  els.terrainMap.querySelectorAll('.terrain-allocation').forEach(allocation => {
    const targetId = allocation.querySelector('[data-target-isopleth]')?.dataset.targetIsopleth;
    const target = els.terrainMap.querySelector(
      `[data-terrain-type="isopleth"][data-terrain-id="${targetId}"] > path`
    );
    if (!target) return;
    const itemLayout = layout.relationship.allocations[allocation.dataset.terrainId];
    const length = target.getTotalLength();
    const candidates = Array.from({ length: 72 }, (_, index) => {
      const point = target.getPointAtLength(length * index / 72);
      return {
        point,
        score: point.y * 3 - Math.abs(point.x - itemLayout.port[0]) * 0.7
      };
    });
    const point = candidates.sort((left, right) => right.score - left.score)[0].point;
    allocation.querySelector('.allocation-anchor').setAttribute('cx', point.x);
    allocation.querySelector('.allocation-anchor').setAttribute('cy', point.y);
    const controlY = Math.max(point.y + 24, (point.y + itemLayout.port[1]) / 2);
    const route = `M${point.x} ${point.y} C${point.x} ${controlY} ${itemLayout.port[0]} ${controlY} ${itemLayout.port[0]} ${itemLayout.port[1]}`;
    allocation.querySelector('.allocation-halo').setAttribute('d', route);
    allocation.querySelector('.allocation-line').setAttribute('d', route);
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
    terrainLayout,
    terrainEndpoint
  };
}

if (typeof document !== 'undefined') {
  document.addEventListener('DOMContentLoaded', initializeTerrain);
}
