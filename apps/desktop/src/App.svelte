<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { CircleStop, Database, Download, Moon, PanelLeft, Play, Sun } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { api, errorMessage, searchRequestLabel, type CollectionJobInfo, type ExportColumn, type ExportFormat, type HealthInfo, type Organization, type ProviderSearchConfig, type ProviderStatus, type ScrapeProgress, type SearchRequest, type SearchRunInfo, type SourceKind } from './lib/ipc';
  import type { CatalogCity, CatalogRubric } from './lib/ipc';
  import LeadTable from './lib/components/LeadTable.svelte';
  import ProviderStatusPanel from './lib/components/ProviderStatusPanel.svelte';
  import AppUpdaterSettings from './lib/components/AppUpdaterSettings.svelte';

  const defaultColumnOrder: ExportColumn[] = ['company', 'category', 'sources', 'address', 'phone', 'inn', 'website', 'tags'];
  const providerConfigs: ProviderSearchConfig[] = [
    { source: 'twoGis', enabled: true, maxResults: 50_000, maxPages: 1000, concurrency: 1, requestDelayMs: 0, preset: 'gentle', maxRetries: 2, backoffBaseSeconds: 60 },
  ];

  let catalogCities: CatalogCity[] = [];
  let selectedCity: CatalogCity | null = null;
  let cityFilter = '';
  let catalogRubrics: CatalogRubric[] = [];
  let selectedRubrics: string[] = [];
  let rubricSearch = '';
  let rubricsLoading = false;
  let rubricsError = '';
  let providerStatuses: ProviderStatus[] = [];
  let rows: Organization[] = [];
  let totalRows = 0;
  let pageOffset = 0;
  const pageSize = 500;
  let runs: SearchRunInfo[] = [];
  let jobs: CollectionJobInfo[] = [];
  let currentRunId: string | null = null;
  let rawRecords = 0;
  let duplicatesMerged = 0;
  let warnings: string[] = [];
  let running = false;
  let message = 'Готово к поиску';
  let error = '';
  let progress = 0;
  let health: HealthInfo | null = null;
  let theme: 'frost' | 'graphite' = 'frost';
  let sidebarCollapsed = false;
  let columnOrder: ExportColumn[] = defaultColumnOrder;

  $: matchingCatalogCities = catalogCities.filter((city) =>
    !cityFilter || city.name.toLowerCase().includes(cityFilter.toLowerCase()));
  $: visibleRubrics = catalogRubrics.filter((rubric) =>
    !rubricSearch || rubric.name.toLowerCase().includes(rubricSearch.toLowerCase()));

  function handleKeydown(event: KeyboardEvent) {
    if (event.metaKey && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      sidebarCollapsed = !sidebarCollapsed;
    }
  }

  onMount(() => {
    try {
      const savedOrder = JSON.parse(localStorage.getItem('lead-column-order') ?? 'null') as unknown;
      if (Array.isArray(savedOrder) && savedOrder.length === defaultColumnOrder.length && savedOrder.every((column) => defaultColumnOrder.includes(column))) {
        columnOrder = [...new Set(savedOrder)] as ExportColumn[];
        if (columnOrder.length !== defaultColumnOrder.length) columnOrder = defaultColumnOrder;
      }
    } catch { columnOrder = defaultColumnOrder; }
    const storedTheme = localStorage.getItem('theme');
    if (storedTheme === 'graphite') theme = 'graphite';
    applyTheme();
    void api.health().then((value) => (health = value)).catch(() => undefined);
    void api.catalogCities().then((value) => {
      catalogCities = value;
      if (!selectedCity && value.length > 0) selectCity(value.find((city) => city.id === '69') ?? value[0]);
    }).catch((cause) => { error = errorMessage(cause); });
    void loadRecentRuns().catch(() => undefined);
    const unlisten = listen<ScrapeProgress>('scrape-progress', ({ payload }) => {
      message = payload.message;
      if (!payload.source) {
        if (payload.phase === 'done') progress = 100;
        else if (payload.total && payload.total > 0) progress = Math.min(99, Math.round((payload.current / payload.total) * 100));
      }
      if (payload.source) {
        const status: ProviderStatus = {
          source: payload.source,
          region: payload.region ?? '',
          state: payload.state ?? 'running',
          current: payload.current,
          total: payload.total,
          message: payload.message,
          retryAfterSeconds: payload.retryAfterSeconds,
        };
        providerStatuses = [...providerStatuses.filter((item) => item.source !== status.source), status];
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  });

  async function selectCity(city: CatalogCity) {
    selectedCity = city;
    selectedRubrics = [];
    catalogRubrics = [];
    rubricsError = '';
    rubricsLoading = true;
    try {
      const value = await api.catalogCityRubrics(city.id);
      if (selectedCity?.id !== city.id) return;
      catalogRubrics = value;
      selectedRubrics = value.map((rubric) => rubric.id);
      if (value.length === 0) rubricsError = 'Для этого города нет доступных выгрузок';
    } catch (cause) {
      if (selectedCity?.id === city.id) rubricsError = errorMessage(cause);
    } finally {
      if (selectedCity?.id === city.id) rubricsLoading = false;
    }
  }

  function toggleRubric(id: string) {
    selectedRubrics = selectedRubrics.includes(id)
      ? selectedRubrics.filter((value) => value !== id)
      : [...selectedRubrics, id];
  }

  function toggleAllRubrics() {
    const visibleIds = visibleRubrics.map((rubric) => rubric.id);
    const allVisibleSelected = visibleIds.length > 0 && visibleIds.every((id) => selectedRubrics.includes(id));
    selectedRubrics = allVisibleSelected
      ? selectedRubrics.filter((id) => !visibleIds.includes(id))
      : [...new Set([...selectedRubrics, ...visibleIds])];
  }

  function applyTheme() {
    document.documentElement.dataset.theme = theme === 'graphite' ? 'graphite' : '';
    localStorage.setItem('theme', theme);
  }
  function toggleTheme() { theme = theme === 'frost' ? 'graphite' : 'frost'; applyTheme(); }
  function runLabel(run: SearchRunInfo) {
    const when = new Date(run.finishedAt).toLocaleString();
    return `${when} · ${searchRequestLabel(run.request)}`;
  }

  async function loadRun(runId: string, offset = 0) {
    const run = runs.find((item) => item.runId === runId);
    const page = await api.resultsForRunPage(runId, offset, pageSize);
    rows = page.items;
    totalRows = page.total;
    pageOffset = page.offset;
    currentRunId = runId;
    if (run) {
      rawRecords = run.rawRecords;
      duplicatesMerged = run.duplicatesMerged;
      warnings = run.warnings;
      message = `${searchRequestLabel(run.request)} · уникальных лидов: ${totalRows.toLocaleString()}`;
    }
  }

  async function loadRecentRuns(selectRunId?: string) {
    [runs, jobs] = await Promise.all([api.recentRuns(30), api.recentCollectionJobs(30)]);
    const target = selectRunId ?? currentRunId ?? runs[0]?.runId;
    if (target) {
      await loadRun(target);
      return;
    }
    rows = await api.recentResults(300);
    totalRows = rows.length;
    currentRunId = null;
    if (rows.length > 0) message = 'Загружены результаты старой версии · выполни новый поиск для экспорта по запуску';
  }

  function buildRequest(): SearchRequest {
    const cityId = selectedCity?.id ?? '';
    return {
      region: cityId,
      regions: [cityId],
      query: (selectedRubrics.length > 0 ? selectedRubrics : catalogRubrics.map((rubric) => rubric.id)).join(','),
      maxResults: 50_000,
      maxPages: 1_000,
      concurrency: 1,
      requestDelayMs: 0,
      sources: ['twoGis'],
      providerConfigs,
    };
  }

  function applySummary(summary: Awaited<ReturnType<typeof api.startSearch>>) {
    currentRunId = summary.runId;
    rows = summary.organizations;
    totalRows = summary.organizationCount;
    pageOffset = 0;
    warnings = summary.warnings;
    rawRecords = summary.rawRecords;
    duplicatesMerged = summary.duplicatesMerged;
    progress = 100;
    message = `Уникальных лидов: ${summary.organizationCount.toLocaleString()} · исходных записей: ${summary.rawRecords.toLocaleString()} · объединено: ${summary.duplicatesMerged.toLocaleString()}`;
  }

  async function collectLeads(request: SearchRequest) {
    running = true; error = ''; warnings = []; providerStatuses = []; progress = 1; message = 'Запуск поиска';
    try {
      const summary = await api.startSearch(request);
      applySummary(summary);
      [runs, jobs] = await Promise.all([api.recentRuns(30), api.recentCollectionJobs(30)]);
    } catch (e) { error = errorMessage(e); message = 'Поиск остановлен'; }
    finally { running = false; }
  }

  async function runSearch() {
    if (running || rubricsLoading || !selectedCity || catalogRubrics.length === 0) return;
    await collectLeads(buildRequest());
  }

  async function resumeJob(jobId: string) {
    running = true; error = ''; providerStatuses = []; message = 'Возобновление сбора';
    try {
      const summary = await api.resumeSearch(jobId);
      applySummary(summary);
      [runs, jobs] = await Promise.all([api.recentRuns(30), api.recentCollectionJobs(30)]);
    } catch (e) { error = errorMessage(e); message = 'Возобновление остановлено'; }
    finally { running = false; }
  }

  async function pauseProvider(source: SourceKind) {
    if (await api.pauseProvider(source)) {
      providerStatuses = providerStatuses.map((item) => item.source === source ? { ...item, state: 'paused' } : item);
    }
  }

  async function resumeProvider(source: SourceKind) {
    if (await api.resumeProvider(source)) {
      providerStatuses = providerStatuses.map((item) => item.source === source ? { ...item, state: 'running' } : item);
    }
  }

  async function loadPage(offset: number) {
    if (!currentRunId) return;
    await loadRun(currentRunId, Math.max(0, offset));
  }
  async function cancelSearch() { await api.cancelSearch(); message = 'Остановка…'; }
  function saveColumnOrder(order: ExportColumn[]) {
    columnOrder = order;
    localStorage.setItem('lead-column-order', JSON.stringify(order));
  }
  async function exportRows(format: ExportFormat) {
    error = '';
    if (!currentRunId) { error = 'Сначала выбери или выполни поиск'; return; }
    try {
      const receipt = await api.exportResults(currentRunId, format);
      if (receipt) message = `Сохранено ${receipt.rows.toLocaleString()}: ${receipt.path}`;
    }
    catch (e) { error = errorMessage(e); }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="shell" class:sidebar-collapsed={sidebarCollapsed}>
  {#if !sidebarCollapsed}
  <aside class="sidebar">
    <div class="brand">
      <button class="icon-button sidebar-toggle" onclick={() => (sidebarCollapsed = true)} aria-label="Скрыть настройки" title="Скрыть настройки (⌘B)"><PanelLeft size={16} /></button>
    </div>

    <div class="form-section">
      <div class="location-field">
        <label for="city-search"><span>Город</span></label>
        <input
          id="city-search"
          value={cityFilter}
          placeholder="Начните вводить название"
          disabled={running}
          oninput={(event) => (cityFilter = event.currentTarget.value)}
        />
        <div class="city-list" role="listbox" aria-label="Города с выгрузками">
          {#each matchingCatalogCities as city (city.id)}
            <button type="button" role="option" aria-selected={selectedCity?.id === city.id} class="city-option" class:active={selectedCity?.id === city.id} onclick={() => void selectCity(city)} disabled={running}>
              <strong>{city.name}</strong>
            </button>
          {:else}
            <div class="no-cities">Нет городов по запросу</div>
          {/each}
        </div>
        {#if selectedCity}<small class="selected-location">{selectedCity.name} · {rubricsLoading ? 'загружаю рубрики…' : `${catalogRubrics.length} рубрик`}</small>{/if}
      </div>

      <div class="location-field">
        <div class="rubrics-heading">
          <label for="rubric-search"><span>Рубрики</span></label>
          <button class="mini-button" type="button" onclick={toggleAllRubrics} disabled={running || catalogRubrics.length === 0}>
            {selectedRubrics.length === visibleRubrics.length && visibleRubrics.length > 0 ? 'Снять все' : 'Выбрать все'}
          </button>
        </div>
        <input
          id="rubric-search"
          value={rubricSearch}
          placeholder="Поиск по рубрикам"
          disabled={running}
          oninput={(event) => (rubricSearch = event.currentTarget.value)}
        />
        <div class="rubric-list" role="group" aria-label="Рубрики города">
          {#if rubricsLoading}
            <div class="no-cities">Загружаю рубрики…</div>
          {:else if rubricsError}
            <div class="no-cities" role="status">{rubricsError}</div>
          {:else}
          {#each visibleRubrics as rubric (rubric.id)}
            <label class="rubric-row">
              <input
                type="checkbox"
                checked={selectedRubrics.includes(rubric.id)}
                disabled={running}
                onchange={() => toggleRubric(rubric.id)}
              />
              <span>{rubric.name}</span>
            </label>
          {:else}
            <div class="no-cities">{selectedCity ? 'Рубрик не найдено' : 'Сначала выберите город'}</div>
          {/each}
          {/if}
        </div>
        {#if selectedRubrics.length > 0}<small class="selected-location">Выбрано рубрик: {selectedRubrics.length}</small>{:else if catalogRubrics.length > 0}<small class="selected-location">Будут загружены все рубрики</small>{/if}
      </div>
    </div>

    <div class="sidebar-actions">
      {#if rubricsError}<small class="sidebar-hint error-text">{rubricsError}</small>{/if}
      {#if running}
        <button class="primary danger" onclick={cancelSearch}><CircleStop size={16} /> Остановить сбор</button>
      {:else}
        <button class="primary" onclick={runSearch} disabled={!selectedCity || rubricsLoading || catalogRubrics.length === 0}><Play size={16} fill="currentColor" /> Собрать лиды</button>
      {/if}
    </div>

    <div class="sidebar-footer">
      <AppUpdaterSettings currentVersion={health?.appVersion} />
      <button class="icon-button" onclick={toggleTheme} title="Сменить тему">{#if theme === 'frost'}<Moon size={16} />{:else}<Sun size={16} />{/if}</button>
    </div>
  </aside>
  {/if}

  <main class="content">
    <header class="topbar">
      <div class="heading">
        {#if sidebarCollapsed}<button class="icon-button sidebar-toggle" onclick={() => (sidebarCollapsed = false)} aria-label="Показать настройки" title="Показать настройки (⌘B)"><PanelLeft size={16} /></button>{/if}
        <div><h1>Лиды</h1><p>{message}</p></div>
      </div>
      <div class="actions">
        {#if runs.length > 0}
          <select
            class="run-select"
            aria-label="История поисков"
            value={currentRunId ?? ''}
            disabled={running}
            onchange={(event) => void loadRun(event.currentTarget.value)}
          >
            {#each runs as run}
              <option value={run.runId}>{runLabel(run)}</option>
            {/each}
          </select>
        {/if}
        <button onclick={() => exportRows('csv')} disabled={totalRows === 0 || running || !currentRunId}><Download size={15} /> CSV…</button>
        <button onclick={() => exportRows('xlsx')} disabled={totalRows === 0 || running || !currentRunId}><Download size={15} /> XLSX…</button>
      </div>
    </header>

    <div class="progress-track" aria-label="Прогресс">
      <div class="progress-value" style={`width: ${progress}%`}></div>
    </div>

    {#if error}
      <div class="notice error">{error}</div>
    {/if}
    {#if warnings.length > 0}
      <div class="notice warning">Предупреждений источников: {warnings.length}. Первый: {warnings[0]}</div>
    {/if}

    <ProviderStatusPanel
      statuses={providerStatuses}
      {jobs}
      {running}
      onPause={(source) => void pauseProvider(source)}
      onResume={(source) => void resumeProvider(source)}
      onResumeJob={(jobId) => void resumeJob(jobId)}
    />

    <section class="stats">
      <div><Database size={16} /><strong>{totalRows}</strong><span>уникальных лидов</span></div>
      <div><strong>{rows.filter((r) => r.dedupe?.possibleDuplicate).length}</strong><span>возможных дублей</span></div>
      <div><strong>{rawRecords}</strong><span>исходных записей</span></div>
      <div><strong>{duplicatesMerged}</strong><span>объединено</span></div>
    </section>

    <LeadTable {rows} {columnOrder} onColumnOrderChange={saveColumnOrder} />
    {#if totalRows > pageSize}
      <div class="pager">
        <button disabled={pageOffset === 0 || running} onclick={() => void loadPage(pageOffset - pageSize)}>Назад</button>
        <span>{(pageOffset + 1).toLocaleString()}–{Math.min(pageOffset + rows.length, totalRows).toLocaleString()} из {totalRows.toLocaleString()}</span>
        <button disabled={pageOffset + pageSize >= totalRows || running} onclick={() => void loadPage(pageOffset + pageSize)}>Далее</button>
      </div>
    {/if}
  </main>
</div>

<style>
  .shell { display: grid; grid-template-columns: 292px minmax(0, 1fr); height: 100%; padding: 12px; gap: 12px; }
  .shell.sidebar-collapsed { grid-template-columns: minmax(0, 1fr); }
  .sidebar, .content { border: 1px solid var(--line); background: var(--panel); backdrop-filter: blur(24px) saturate(130%); box-shadow: var(--shadow); }
  .sidebar { border-radius: 18px; padding: 16px; display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
  .content { border-radius: 18px; min-width: 0; min-height: 0; overflow: hidden; display: flex; flex-direction: column; }
  .brand { display: flex; gap: 10px; align-items: center; padding: 2px 2px 18px; border-bottom: 1px solid var(--line); }
  .form-section { display: grid; align-content: start; gap: 12px; padding: 18px 2px 12px; min-height: 0; flex: 1; overflow-y: auto; }
  .location-field { min-width: 0; }
  .location-field > label { display:block; }
  .selected-location { color:var(--muted); font-size:10px; }
  .selected-location { display:block; margin:5px 2px 0; }
  label span { display: block; color: var(--muted); font-size: 11px; font-weight: 650; margin: 0 0 6px 2px; text-transform: uppercase; letter-spacing: 0.055em; }
  input { width: 100%; height: 36px; border: 1px solid var(--line); background: var(--panel-solid); color: var(--fg); border-radius: 9px; padding: 0 10px; outline: none; transition: border 120ms ease, box-shadow 120ms ease; }
  input:focus { border-color: color-mix(in srgb, var(--fg) 28%, transparent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--fg) 7%, transparent); }
  button { height: 34px; border: 1px solid var(--line); border-radius: 9px; background: var(--panel-solid); color: var(--fg); padding: 0 11px; display: inline-flex; align-items: center; justify-content: center; gap: 7px; font-size: 12px; font-weight: 600; }
  button:disabled { opacity: .45; }
  button:not(:disabled):active { transform: translateY(1px); }
  .primary { margin-top: 4px; width: 100%; height: 38px; background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  .danger { background: color-mix(in srgb, var(--danger) 86%, #111); color: white; }
  .sidebar-actions { padding: 4px 0 10px; border-top: 1px solid var(--line); flex: 0 0 auto; }
  .sidebar-hint { display:block; margin:0 0 7px; font-size:10px; line-height:1.4; }
  .error-text { color:var(--danger); }
  .sidebar-footer { padding-top: 10px; border-top: 1px solid var(--line); display: flex; align-items: center; gap:8px; color: var(--muted); font-size: 11px; flex:0 0 auto; }
  .icon-button { width: 32px; padding: 0; }
  .heading { display:flex; align-items:center; gap:10px; min-width:0; }
  .sidebar-toggle { flex:0 0 auto; }
  .topbar { display: flex; justify-content: space-between; gap: 16px; align-items: center; padding: 18px 20px 14px; }
  h1 { font-size: 18px; line-height: 1.1; letter-spacing: -0.025em; margin: 0; }
  .topbar p { color: var(--muted); font-size: 11px; margin: 5px 0 0; max-width: 640px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 6px; align-items: center; }
  .run-select { max-width: 260px; height: 34px; border: 1px solid var(--line); border-radius: 9px; background: var(--panel-solid); color: var(--fg); padding: 0 9px; font-size: 11px; }
  .progress-track { height: 2px; background: var(--panel-muted); }
  .progress-value { height: 100%; background: var(--accent); transition: width 180ms ease; }
  .notice { margin: 12px 18px 0; border-radius: 9px; padding: 9px 11px; font-size: 11px; border: 1px solid var(--line); }
  .notice.error { color: var(--danger); background: color-mix(in srgb, var(--danger) 7%, var(--panel-solid)); }
  .notice.warning { color: var(--warning); background: color-mix(in srgb, var(--warning) 7%, var(--panel-solid)); }
  .stats { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; padding: 14px 18px; }
  .stats > div { min-width: 0; height: 54px; border: 1px solid var(--line); border-radius: 11px; background: var(--panel-solid); padding: 10px 9px; display: flex; align-items: center; gap: 7px; }
  .stats strong { font-size: 17px; letter-spacing: -0.04em; }
  .stats span { color: var(--muted); font-size: 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pager { display:flex; justify-content:center; align-items:center; gap:10px; padding:0 18px 14px; color:var(--muted); font-size:10px; }
  .pager button { height:28px; }
  @media (max-width: 980px) { .shell { grid-template-columns: 252px minmax(0, 1fr); } .stats { grid-template-columns: repeat(2, 1fr); } }

  .city-list, .rubric-list { max-height: 180px; overflow-y: auto; border: 1px solid var(--line); border-radius: 9px; background: var(--panel-solid); padding: 4px; display: grid; gap: 2px; }
  .rubric-list { max-height: 220px; }
  .city-option { display: block; width: 100%; min-height: 30px; padding: 4px 8px; border: 0; border-radius: 6px; text-align: left; background: transparent; color: var(--fg); font-size: 11px; cursor: pointer; }
  .city-option strong { font-weight: 500; }
  .city-option:hover, .city-option.active { background: var(--panel-muted); }
  .city-option.active strong { font-weight: 700; }
  .rubric-row { display: flex; align-items: center; gap: 7px; min-height: 26px; padding: 2px 6px; border-radius: 6px; font-size: 11px; cursor: pointer; }
  .rubric-row:hover { background: var(--panel-muted); }
  .rubric-row input { width: 14px; height: 14px; margin: 0; accent-color: var(--fg); }
  .rubric-row span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .no-cities { padding: 10px; color: var(--muted); font-size: 10px; text-align: center; }
  .rubrics-heading { display: flex; align-items: center; justify-content: space-between; }
  .mini-button { height: 22px; padding: 0 8px; font-size: 9px; border: 1px solid var(--line); border-radius: 6px; background: var(--panel-solid); color: var(--muted); cursor: pointer; }
  .mini-button:disabled { opacity: .45; cursor: default; }
  @media (prefers-reduced-motion: reduce) { * { scroll-behavior:auto !important; transition-duration:0.01ms !important; } }
</style>
