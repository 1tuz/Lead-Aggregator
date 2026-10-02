<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { CircleStop, Database, Download, Moon, PanelLeft, Play, Sun } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { api, errorMessage, type ExportFormat, type HealthInfo, type Organization, type SearchRunInfo, type SourceKind } from './lib/ipc';
  import { citySlug, normalizePlaceName, regionCityCount, russianCities, russianRegions, type RussianCity } from './lib/geography';
  import LeadTable from './lib/components/LeadTable.svelte';

  type Progress = { phase: 'discovering' | 'enriching' | 'resolving' | 'saving' | 'done'; current: number; total: number | null; message: string };
  const sourceOptions: { id: SourceKind; label: string; note: string }[] = [
    { id: 'twoGis', label: '2GIS', note: 'локальный бизнес' },
    { id: 'yell', label: 'Yell', note: 'каталог компаний' },
    { id: 'zoon', label: 'Zoon', note: 'услуги и малый бизнес' },
    { id: 'rusprofile', label: 'Rusprofile', note: 'юрлица и реквизиты' },
  ];

  let region = '';
  let locationText = 'Все регионы';
  let selectedCity: RussianCity | null = null;
  let regionFilter = '';
  let allRegionsMode = true;
  let locationSuggestionsOpen = false;
  let activeSuggestion = 0;
  let locationPicker: HTMLDivElement;
  let query = 'автосервис';
  let maxResults = 100;
  let maxPages = 5;
  let concurrency = 2;
  let delay = 650;
  let selectedSources: SourceKind[] = ['twoGis', 'yell'];
  let rows: Organization[] = [];
  let runs: SearchRunInfo[] = [];
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

  $: normalizedLocation = normalizePlaceName(locationText);
  $: matchingRegions = regionFilter
    ? []
    : allRegionsMode
      ? russianRegions
      : russianRegions.filter((name) => normalizePlaceName(name).includes(normalizedLocation)).slice(0, 5);
  $: matchingCities = russianCities
    .filter((city) => (!regionFilter || city.region === regionFilter || city.autonomousDistrict === regionFilter)
      && !allRegionsMode
      && (!normalizedLocation || normalizePlaceName(`${city.name} ${city.region} ${city.autonomousDistrict ?? ''}`).includes(normalizedLocation)))
    .slice(0, regionFilter ? 1000 : 10);
  $: suggestions = [
    { type: 'all' as const },
    ...matchingRegions.map((name) => ({ type: 'region' as const, name })),
    ...matchingCities.map((city) => ({ type: 'city' as const, city })),
  ];

  function handleKeydown(event: KeyboardEvent) {
    if (locationSuggestionsOpen && suggestions.length > 0) {
      if (event.key === 'ArrowDown') { event.preventDefault(); activeSuggestion = (activeSuggestion + 1) % suggestions.length; return; }
      if (event.key === 'ArrowUp') { event.preventDefault(); activeSuggestion = (activeSuggestion + suggestions.length - 1) % suggestions.length; return; }
      if (event.key === 'Enter' && suggestions[activeSuggestion]) {
        event.preventDefault();
        chooseLocation(suggestions[activeSuggestion]);
        return;
      }
    }
    if (event.key === 'Escape') locationSuggestionsOpen = false;
    if (event.metaKey && event.key.toLowerCase() === 'b') {
      event.preventDefault();
      sidebarCollapsed = !sidebarCollapsed;
    }
  }

  onMount(() => {
    const storedTheme = localStorage.getItem('theme');
    if (storedTheme === 'graphite') theme = 'graphite';
    applyTheme();
    void api.health().then((value) => (health = value)).catch(() => undefined);
    void loadRecentRuns().catch(() => undefined);
    const unlisten = listen<Progress>('scrape-progress', ({ payload }) => {
      message = payload.message;
      if (payload.total && payload.total > 0) progress = Math.min(100, Math.round((payload.current / payload.total) * 100));
      else if (payload.phase === 'done') progress = 100;
    });
    const closeLocationSuggestions = (event: PointerEvent) => {
      if (locationPicker && !locationPicker.contains(event.target as Node)) locationSuggestionsOpen = false;
    };
    document.addEventListener('pointerdown', closeLocationSuggestions);
    return () => {
      void unlisten.then((fn) => fn());
      document.removeEventListener('pointerdown', closeLocationSuggestions);
    };
  });

  function chooseLocation(option: (typeof suggestions)[number]) {
    if (option.type === 'all') {
      selectedCity = null;
      region = '';
      regionFilter = '';
      allRegionsMode = true;
      locationText = 'Все регионы';
      activeSuggestion = 0;
      locationSuggestionsOpen = true;
      return;
    }
    if (option.type === 'region') {
      allRegionsMode = false;
      regionFilter = option.name;
      locationText = '';
      activeSuggestion = 0;
      locationSuggestionsOpen = true;
      return;
    }
    selectedCity = option.city;
    allRegionsMode = false;
    region = citySlug(option.city.name);
    regionFilter = '';
    locationText = option.city.name;
    locationSuggestionsOpen = false;
  }

  function applyTheme() {
    document.documentElement.dataset.theme = theme === 'graphite' ? 'graphite' : '';
    localStorage.setItem('theme', theme);
  }
  function toggleTheme() { theme = theme === 'frost' ? 'graphite' : 'frost'; applyTheme(); }
  function toggleSource(source: SourceKind) {
    selectedSources = selectedSources.includes(source)
      ? selectedSources.filter((item) => item !== source)
      : [...selectedSources, source];
  }
  function runLabel(run: SearchRunInfo) {
    const when = new Date(run.finishedAt).toLocaleString();
    return `${when} · ${run.request.query}`;
  }

  async function loadRun(runId: string) {
    const run = runs.find((item) => item.runId === runId);
    rows = await api.resultsForRun(runId, 5000);
    currentRunId = runId;
    if (run) {
      rawRecords = run.rawRecords;
      duplicatesMerged = run.duplicatesMerged;
      warnings = run.warnings;
      message = `${run.request.query} · уникальных лидов: ${rows.length}`;
    }
  }

  async function loadRecentRuns(selectRunId?: string) {
    runs = await api.recentRuns(30);
    const target = selectRunId ?? currentRunId ?? runs[0]?.runId;
    if (target) {
      await loadRun(target);
      return;
    }
    rows = await api.recentResults(300);
    currentRunId = null;
    if (rows.length > 0) message = 'Загружены результаты старой версии · выполни новый поиск для экспорта по запуску';
  }

  async function runSearch() {
    running = true; error = ''; warnings = []; progress = 1; message = 'Запуск поиска';
    try {
      const summary = await api.startSearch({ region, query, maxResults, maxPages, concurrency, requestDelayMs: delay, sources: selectedSources });
      currentRunId = summary.runId;
      rows = summary.organizations; warnings = summary.warnings; progress = 100;
      rawRecords = summary.rawRecords; duplicatesMerged = summary.duplicatesMerged;
      runs = await api.recentRuns(30);
      message = `Уникальных лидов: ${rows.length} · исходных записей: ${summary.rawRecords} · объединено: ${summary.duplicatesMerged}`;
    } catch (e) { error = errorMessage(e); message = 'Поиск остановлен'; }
    finally { running = false; }
  }
  async function cancelSearch() { await api.cancelSearch(); message = 'Остановка…'; }
  async function exportRows(format: ExportFormat) {
    error = '';
    if (!currentRunId) { error = 'Сначала выбери или выполни поиск'; return; }
    try { const receipt = await api.exportResults(currentRunId, format, 5000); message = `Экспортировано ${receipt.rows}: ${receipt.path}`; }
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
        <label for="location-search"><span>Город или регион</span></label>
        <div class="location-picker" bind:this={locationPicker}>
          <input
            id="location-search"
            value={locationText}
            placeholder="Начните вводить название"
            disabled={running}
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={locationSuggestionsOpen}
            aria-controls="location-suggestions"
            onfocus={() => (locationSuggestionsOpen = true)}
            oninput={(event) => {
              locationText = event.currentTarget.value;
              allRegionsMode = false;
              selectedCity = null;
              region = '';
              activeSuggestion = 0;
              locationSuggestionsOpen = true;
            }}
          />
          {#if regionFilter}
            <div class="region-filter"><span>{regionFilter}</span><button type="button" aria-label="Очистить фильтр региона" onclick={() => { regionFilter = ''; locationText = ''; }}>×</button></div>
          {/if}
          {#if locationSuggestionsOpen && suggestions.length > 0}
            <div class="location-suggestions" id="location-suggestions" role="listbox" aria-label="Города и регионы России">
              {#each suggestions as suggestion, index (suggestion.type === 'all' ? 'all-regions' : suggestion.type === 'region' ? `region-${suggestion.name}` : `city-${suggestion.city.name}-${suggestion.city.region}`)}
                {#if suggestion.type === 'all'}
                  <button type="button" role="option" aria-selected={activeSuggestion === index} class="location-option region-option" class:active={activeSuggestion === index} onclick={() => chooseLocation(suggestion)}>
                    <strong>Все регионы</strong><small>{russianRegions.length} регионов России</small>
                  </button>
                {:else if suggestion.type === 'region'}
                  <button type="button" role="option" aria-selected={activeSuggestion === index} class="location-option region-option" class:active={activeSuggestion === index} onclick={() => chooseLocation(suggestion)}>
                    <strong>{suggestion.name}</strong><small>{regionCityCount(suggestion.name)} городов · показать</small>
                  </button>
                {:else}
                  <button type="button" role="option" aria-selected={activeSuggestion === index} class="location-option" class:active={activeSuggestion === index} onclick={() => chooseLocation(suggestion)}>
                    <strong>{suggestion.city.name}</strong><small>{suggestion.city.autonomousDistrict ? `${suggestion.city.autonomousDistrict} · ` : ''}{suggestion.city.region}</small>
                  </button>
                {/if}
              {/each}
            </div>
          {:else if locationSuggestionsOpen && (normalizedLocation || regionFilter)}
            <div class="location-suggestions no-suggestions">Ничего не найдено</div>
          {/if}
        </div>
        {#if selectedCity}<small class="selected-location">{selectedCity.region}{selectedCity.autonomousDistrict ? ` · ${selectedCity.autonomousDistrict}` : ''}</small>{/if}
      </div>
      <label><span>Что искать</span><input bind:value={query} placeholder="автосервис" disabled={running} /></label>

      <details class="settings-disclosure">
        <summary>Настройки сбора</summary>
        <div class="settings-content">
          <div class="source-block">
            <span class="field-title">Источники</span>
            {#each sourceOptions as source}
              <label class="source-row">
                <input type="checkbox" checked={selectedSources.includes(source.id)} onchange={() => toggleSource(source.id)} disabled={running || (health ? !health.availableSources.includes(source.id) : false)} />
                <span><strong>{source.label}</strong><small>{source.note}</small></span>
              </label>
            {/each}
          </div>

          <div class="field-grid">
            <label><span>Макс. на источник</span><input type="number" min="1" max="5000" bind:value={maxResults} disabled={running} /></label>
            <label><span>Страниц</span><input type="number" min="1" max="100" bind:value={maxPages} disabled={running} /></label>
          </div>
          <div class="field-grid">
            <label><span>Параллельно</span><input type="number" min="1" max="8" bind:value={concurrency} disabled={running} /></label>
            <label><span>Пауза, мс</span><input type="number" min="250" step="50" bind:value={delay} disabled={running} /></label>
          </div>
        </div>
      </details>

      {#if running}
        <button class="primary danger" onclick={cancelSearch}><CircleStop size={16} /> Остановить</button>
      {:else}
        <button class="primary" onclick={runSearch} disabled={!query.trim() || !selectedCity || !region.trim() || selectedSources.length === 0}><Play size={16} fill="currentColor" /> Собрать лиды</button>
      {/if}
    </div>

    <div class="sidebar-footer">
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
        <button onclick={() => exportRows('csv')} disabled={rows.length === 0 || running || !currentRunId}><Download size={15} /> CSV</button>
        <button onclick={() => exportRows('xlsx')} disabled={rows.length === 0 || running || !currentRunId}><Download size={15} /> XLSX</button>
        <button onclick={() => exportRows('json')} disabled={rows.length === 0 || running || !currentRunId}><Download size={15} /> JSON</button>
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

    <section class="stats">
      <div><Database size={16} /><strong>{rows.length}</strong><span>уникальных лидов</span></div>
      <div><strong>{rows.filter((r) => r.sources.length > 1).length}</strong><span>из 2+ источников</span></div>
      <div><strong>{rows.filter((r) => r.dedupe.possibleDuplicate).length}</strong><span>возможных дублей</span></div>
      <div><strong>{rawRecords}</strong><span>исходных записей</span></div>
      <div><strong>{duplicatesMerged}</strong><span>объединено</span></div>
    </section>

    <LeadTable {rows} />
  </main>
</div>

<style>
  .shell { display: grid; grid-template-columns: 292px minmax(0, 1fr); height: 100%; padding: 12px; gap: 12px; }
  .shell.sidebar-collapsed { grid-template-columns: minmax(0, 1fr); }
  .sidebar, .content { border: 1px solid var(--line); background: var(--panel); backdrop-filter: blur(24px) saturate(130%); box-shadow: var(--shadow); }
  .sidebar { border-radius: 18px; padding: 16px; display: flex; flex-direction: column; min-height: 0; overflow-y: auto; }
  .content { border-radius: 18px; min-width: 0; min-height: 0; overflow: hidden; display: flex; flex-direction: column; }
  .brand { display: flex; gap: 10px; align-items: center; padding: 2px 2px 18px; border-bottom: 1px solid var(--line); }
  .form-section { display: grid; gap: 12px; padding-top: 18px; }
  .location-field { min-width: 0; }
  .location-field > label { display:block; }
  .location-picker { position:relative; }
  .location-picker input { display:block; }
  .location-suggestions { position:absolute; z-index:5; top:calc(100% + 5px); left:0; right:0; max-height:300px; overflow-y:auto; padding:4px; border:1px solid var(--line); border-radius:11px; background:var(--panel-solid); box-shadow:var(--shadow); }
  .location-option { display:flex; flex-direction:column; align-items:flex-start; width:100%; min-height:42px; padding:7px 9px; border:0; border-radius:7px; text-align:left; }
  .location-option strong { color:var(--fg); font-size:12px; font-weight:600; }
  .location-option small, .selected-location { color:var(--muted); font-size:10px; }
  .location-option.active, .location-option:hover { background:var(--panel-muted); }
  .region-option { border-bottom:1px solid var(--line); border-radius:7px 7px 3px 3px; }
  .region-filter { display:flex; align-items:center; justify-content:space-between; gap:8px; margin-top:5px; padding:4px 8px; border-radius:7px; color:var(--muted); background:var(--panel-muted); font-size:10px; }
  .region-filter button { width:20px; height:20px; padding:0; border:0; }
  .selected-location { display:block; margin:5px 2px 0; }
  .no-suggestions { padding:12px; color:var(--muted); font-size:11px; }
  label span { display: block; color: var(--muted); font-size: 11px; font-weight: 650; margin: 0 0 6px 2px; text-transform: uppercase; letter-spacing: 0.055em; }
  input { width: 100%; height: 36px; border: 1px solid var(--line); background: var(--panel-solid); color: var(--fg); border-radius: 9px; padding: 0 10px; outline: none; transition: border 120ms ease, box-shadow 120ms ease; }
  input:focus { border-color: color-mix(in srgb, var(--fg) 28%, transparent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--fg) 7%, transparent); }
  .field-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .settings-disclosure { border: 1px solid var(--line); border-radius: 11px; background: var(--panel-muted); padding: 0 10px; }
  .settings-disclosure summary { min-height: 38px; display:flex; align-items:center; color:var(--fg); font-size:11px; font-weight:650; cursor:pointer; list-style:none; }
  .settings-disclosure summary::-webkit-details-marker { display:none; }
  .settings-disclosure summary::after { content:""; width:7px; height:7px; margin:0 2px 4px auto; border-right:1.5px solid var(--muted); border-bottom:1.5px solid var(--muted); transform:rotate(45deg); transition:transform 140ms ease; }
  .settings-disclosure[open] summary::after { margin-bottom:-4px; transform:rotate(225deg); }
  .settings-content { display:grid; gap:10px; padding:0 0 10px; }
  .field-title { display:block; color:var(--muted); font-size:10px; font-weight:650; margin:0 0 2px 2px; text-transform:uppercase; letter-spacing:.055em; }
  .source-block { border:1px solid var(--line); background:var(--panel-muted); border-radius:11px; padding:9px 10px; display:grid; gap:3px; }
  .source-row { display:flex; align-items:center; gap:8px; min-height:28px; margin:0; cursor:pointer; }
  .source-row input[type='checkbox'] { flex:0 0 15px; width:15px; height:15px; margin:0; padding:0; accent-color:var(--fg); }
  .source-row span { display:flex; align-items:baseline; justify-content:space-between; gap:8px; width:100%; margin:0; text-transform:none; letter-spacing:0; font-size:11px; }
  .source-row strong { color:var(--fg); font-size:11px; font-weight:650; }
  .source-row small { color:var(--muted); font-size:9px; font-weight:400; white-space:nowrap; }
  button { height: 34px; border: 1px solid var(--line); border-radius: 9px; background: var(--panel-solid); color: var(--fg); padding: 0 11px; display: inline-flex; align-items: center; justify-content: center; gap: 7px; font-size: 12px; font-weight: 600; }
  button:disabled { opacity: .45; }
  button:not(:disabled):active { transform: translateY(1px); }
  .primary { margin-top: 4px; width: 100%; height: 38px; background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  .danger { background: color-mix(in srgb, var(--danger) 86%, #111); color: white; }
  .sidebar-footer { margin-top: auto; padding-top: 14px; border-top: 1px solid var(--line); display: flex; align-items: center; justify-content: space-between; color: var(--muted); font-size: 11px; }
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
  .stats { grid-template-columns: repeat(5, minmax(0, 1fr)); }
  .stats > div { min-width: 0; height: 54px; border: 1px solid var(--line); border-radius: 11px; background: var(--panel-solid); padding: 10px 9px; display: flex; align-items: center; gap: 7px; }
  .stats strong { font-size: 17px; letter-spacing: -0.04em; }
  .stats span { color: var(--muted); font-size: 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  @media (max-width: 980px) { .shell { grid-template-columns: 252px minmax(0, 1fr); } .stats { grid-template-columns: repeat(2, 1fr); } }
</style>
