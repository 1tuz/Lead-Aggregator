<script lang="ts">
  import type { CollectionPreset, ProviderSearchConfig, SourceKind } from '../ipc';

  let {
    configs,
    running,
    availableSources,
    onChange,
  }: {
    configs: ProviderSearchConfig[];
    running: boolean;
    availableSources: SourceKind[];
    onChange: (configs: ProviderSearchConfig[]) => void;
  } = $props();

  const labels: Record<SourceKind, string> = {
    twoGis: '2GIS',
    yell: 'Yell',
    zoon: 'Zoon',
    rusprofile: 'Rusprofile',
  };

  const safeMinimums: Record<SourceKind, { delay: number; concurrency: number }> = {
    twoGis: { delay: 250, concurrency: 1 },
    yell: { delay: 2000, concurrency: 1 },
    zoon: { delay: 3000, concurrency: 1 },
    rusprofile: { delay: 4500, concurrency: 1 },
  };

  function recommended(source: SourceKind, preset: Exclude<CollectionPreset, 'custom'>): ProviderSearchConfig {
    const table: Record<SourceKind, Record<'gentle' | 'normal', Omit<ProviderSearchConfig, 'source' | 'enabled' | 'preset'>>> = {
      twoGis: {
        gentle: { maxResults: 500, maxPages: 50, concurrency: 1, requestDelayMs: 250, maxRetries: 2, backoffBaseSeconds: 60 },
        normal: { maxResults: 1000, maxPages: 100, concurrency: 1, requestDelayMs: 250, maxRetries: 1, backoffBaseSeconds: 45 },
      },
      yell: {
        gentle: { maxResults: 1500, maxPages: 50, concurrency: 1, requestDelayMs: 2500, maxRetries: 2, backoffBaseSeconds: 90 },
        normal: { maxResults: 7500, maxPages: 500, concurrency: 1, requestDelayMs: 2000, maxRetries: 1, backoffBaseSeconds: 60 },
      },
      zoon: {
        gentle: { maxResults: 1000, maxPages: 40, concurrency: 1, requestDelayMs: 3500, maxRetries: 2, backoffBaseSeconds: 120 },
        normal: { maxResults: 5000, maxPages: 400, concurrency: 1, requestDelayMs: 3000, maxRetries: 1, backoffBaseSeconds: 90 },
      },
      rusprofile: {
        gentle: { maxResults: 1000, maxPages: 50, concurrency: 1, requestDelayMs: 5000, maxRetries: 2, backoffBaseSeconds: 180 },
        normal: { maxResults: 10000, maxPages: 1000, concurrency: 1, requestDelayMs: 4500, maxRetries: 1, backoffBaseSeconds: 120 },
      },
    };
    const current = configs.find((config) => config.source === source);
    return {
      source,
      enabled: current?.enabled ?? false,
      preset,
      ...table[source][preset],
    };
  }

  function patch(source: SourceKind, values: Partial<ProviderSearchConfig>, custom = true) {
    onChange(configs.map((config) => config.source === source
      ? { ...config, ...values, ...(custom ? { preset: 'custom' as CollectionPreset } : {}) }
      : config));
  }

  function setPreset(source: SourceKind, preset: CollectionPreset) {
    if (preset === 'custom') {
      patch(source, { preset }, false);
      return;
    }
    const next = recommended(source, preset);
    onChange(configs.map((config) => config.source === source ? next : config));
  }
</script>

<div class="providers">
  {#each configs as config (config.source)}
    <details class="provider" open={config.enabled}>
      <summary>
        <input
          type="checkbox"
          checked={config.enabled}
          disabled={running || !availableSources.includes(config.source)}
          onclick={(event) => event.stopPropagation()}
          onchange={(event) => patch(config.source, { enabled: event.currentTarget.checked }, false)}
        />
        <strong>{labels[config.source]}</strong>
        <small>мин. {safeMinimums[config.source].delay} мс · до {safeMinimums[config.source].concurrency} параллельно</small>
      </summary>

      <div class="provider-body">
        <label class="wide">
          <span>Режим</span>
          <select value={config.preset} disabled={running} onchange={(event) => setPreset(config.source, event.currentTarget.value as CollectionPreset)}>
            <option value="gentle">Бережный</option>
            <option value="normal">Обычный</option>
            <option value="custom">Пользовательский</option>
          </select>
        </label>
        <label><span>Лимит</span><input type="number" min="1" max="50000" value={config.maxResults} disabled={running} onchange={(event) => patch(config.source, { maxResults: Number(event.currentTarget.value) })} /></label>
        <label><span>{config.source === 'twoGis' ? 'Запросов API' : 'Страниц'}</span><input type="number" min="1" max={config.source === 'twoGis' ? 100 : 5000} value={config.maxPages} disabled={running} onchange={(event) => patch(config.source, { maxPages: Number(event.currentTarget.value) })} /></label>
        <label><span>Пауза, мс</span><input type="number" min="250" step="50" value={config.requestDelayMs} disabled={running} onchange={(event) => patch(config.source, { requestDelayMs: Number(event.currentTarget.value) })} /></label>
        <label><span>Параллельно</span><input type="number" min="1" max="8" value={config.concurrency} disabled={running} onchange={(event) => patch(config.source, { concurrency: Number(event.currentTarget.value) })} /></label>
      </div>
    </details>
  {/each}
</div>

<style>
  .providers { display:grid; gap:7px; }
  .provider { border:1px solid var(--line); border-radius:10px; background:var(--panel-muted); overflow:hidden; }
  summary { min-height:38px; display:grid; grid-template-columns:16px auto 1fr; align-items:center; gap:8px; padding:0 9px; cursor:pointer; list-style:none; }
  summary::-webkit-details-marker { display:none; }
  summary input { width:15px; height:15px; margin:0; accent-color:var(--fg); }
  summary strong { font-size:11px; }
  summary small { color:var(--muted); font-size:9px; text-align:right; }
  .provider-body { display:grid; grid-template-columns:1fr 1fr; gap:7px; padding:8px 9px 10px; border-top:1px solid var(--line); }
  label span { display:block; margin:0 0 5px 2px; color:var(--muted); font-size:9px; text-transform:uppercase; letter-spacing:.045em; }
  input, select { width:100%; height:32px; border:1px solid var(--line); border-radius:8px; background:var(--panel-solid); color:var(--fg); padding:0 8px; font-size:11px; }
  .wide { grid-column:1 / -1; }
</style>
