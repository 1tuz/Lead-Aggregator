<script lang="ts">
  import type { Organization, SourceKind } from '../ipc';

  let { rows }: { rows: Organization[] } = $props();

  function sourceLabel(source: SourceKind) {
    switch (source) {
      case 'twoGis': return '2GIS';
      case 'yell': return 'Yell';
      case 'zoon': return 'Zoon';
      case 'rusprofile': return 'Rusprofile';
    }
  }
</script>

<section class="table-card">
  <div class="table-scroll">
    <table>
      <thead>
        <tr>
          <th>Компания</th>
          <th>Источники</th>
          <th>Адрес</th>
          <th>Телефон</th>
          <th>ИНН</th>
          <th>Сайт / почта</th>
          <th>Метки</th>
        </tr>
      </thead>
      <tbody>
        {#if rows.length === 0}
          <tr class="empty-row"><td colspan="7">Запусти поиск или выбери сохранённый запуск.</td></tr>
        {:else}
          {#each rows as row (row.id)}
            <tr>
              <td><strong>{row.name}</strong><small class="subline">{row.category ?? '—'}</small></td>
              <td><div class="chips">{#each row.sources as source}<span class="chip">{sourceLabel(source.source)}</span>{/each}</div></td>
              <td class="address">{row.address ?? '—'}</td>
              <td>{row.phones[0] ?? '—'}</td>
              <td>{row.inn ?? '—'}</td>
              <td>{row.website ?? row.email ?? '—'}</td>
              <td><div class="chips">{#each row.tags.slice(0, 3) as tag}<span class:warn-chip={tag === 'Возможный дубль'} class="chip">{tag}</span>{/each}</div></td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>
</section>

<style>
  .table-card { min-height: 0; flex: 1; margin: 0 18px 18px; border: 1px solid var(--line); border-radius: 12px; overflow: hidden; background: var(--panel-solid); }
  .table-scroll { width: 100%; height: 100%; overflow: auto; }
  table { width: 100%; border-collapse: collapse; table-layout: fixed; font-size: 11px; }
  th { position: sticky; top: 0; z-index: 1; background: color-mix(in srgb, var(--panel-solid) 94%, var(--panel-muted)); color: var(--muted); text-align: left; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; font-weight: 700; }
  th, td { padding: 10px 12px; border-bottom: 1px solid var(--line); vertical-align: middle; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  th:nth-child(1) { width: 22%; } th:nth-child(2) { width: 16%; } th:nth-child(3) { width: 25%; } th:nth-child(4) { width: 12%; } th:nth-child(5) { width: 13%; } th:nth-child(6) { width: 17%; }
  tbody tr:hover { background: color-mix(in srgb, var(--fg) 3%, transparent); }
  td.address { color: var(--muted); }
  .subline { display:block; color:var(--muted); margin-top:3px; overflow:hidden; text-overflow:ellipsis; }
  .chips { display:flex; gap:4px; overflow:hidden; }
  .chip { display:inline-flex; height:20px; align-items:center; border:1px solid var(--line); border-radius:999px; padding:0 7px; font-size:9px; background:var(--panel-muted); }
  .warn-chip { color:var(--warning); }
  .empty-row td { text-align: center; color: var(--muted); padding: 48px 16px; }
  @media (max-width: 980px) { th:nth-child(2), td:nth-child(2) { display:none; } }
</style>
