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
    twoGis: '2GIS · выгрузки',
  };

  const safeMinimums: Record<SourceKind, { delay: number; concurrency: number }> = {
    twoGis: { delay: 0, concurrency: 1 },
  };

  function recommended(source: SourceKind, preset: Exclude<CollectionPreset, 'custom'>): ProviderSearchConfig {
    const table: Record<SourceKind, Record<'gentle' | 'normal', Omit<ProviderSearchConfig, 'source' | 'enabled' | 'preset'>>> = {
      twoGis: {
        gentle: { maxResults: 50_000, maxPages: 1000, concurrency: 1, requestDelayMs: 0, maxRetries: 2, backoffBaseSeconds: 60 },
        normal: { maxResults: 50_000, maxPages: 2000, concurrency: 1, requestDelayMs: 0, maxRetries: 1, backoffBaseSeconds: 45 },
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
        <small>без пауз · выгрузки статичны</small>
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
        <label><span>Пауза, мс</span><input type="number" min="0" step="50" value={config.requestDelayMs} disabled={running} onchange={(event) => patch(config.source, { requestDelayMs: Number(event.currentTarget.value) })} /></label>
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
