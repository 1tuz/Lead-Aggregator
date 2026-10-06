<script lang="ts">
  import type { ExportColumn, Organization, SourceKind } from '../ipc';

  let {
    rows,
    columnOrder,
    onColumnOrderChange,
  }: {
    rows: Organization[];
    columnOrder: ExportColumn[];
    onColumnOrderChange: (order: ExportColumn[]) => void;
  } = $props();
  let draggedColumn: ExportColumn | null = $state(null);

  const labels: Record<ExportColumn, string> = {
    company: 'Компания',
    category: 'Категория',
    sources: 'Источники',
    address: 'Адрес',
    phone: 'Телефон',
    inn: 'ИНН',
    website: 'Сайт / почта',
    tags: 'Метки',
  };

  function sourceLabel(source: SourceKind) {
    switch (source) {
      case 'twoGis': return '2GIS';
    }
  }

  function moveColumn(target: ExportColumn) {
    if (!draggedColumn || draggedColumn === target) return;
    const next = [...columnOrder];
    const from = next.indexOf(draggedColumn);
    const to = next.indexOf(target);
    if (from < 0 || to < 0) return;
    next.splice(from, 1);
    next.splice(from < to ? to - 1 : to, 0, draggedColumn);
    onColumnOrderChange(next);
    draggedColumn = null;
  }
</script>

<section class="table-card">
  <div class="table-scroll">
    <table>
      <thead>
        <tr>
          {#each columnOrder as column (column)}
            <th
              draggable="true"
              ondragstart={() => (draggedColumn = column)}
              ondragend={() => (draggedColumn = null)}
              ondragover={(event) => event.preventDefault()}
              ondrop={(event) => { event.preventDefault(); moveColumn(column); }}
              class:dragging={draggedColumn === column}
              title="Перетащите, чтобы изменить порядок"
              class={`col-${column}`}
            >{labels[column]} <span aria-hidden="true">⠿</span></th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#if rows.length === 0}
          <tr class="empty-row"><td colspan={columnOrder.length}>Запусти поиск или выбери сохранённый запуск.</td></tr>
        {:else}
          {#each rows as row (row.id)}
            <tr>
              {#each columnOrder as column (column)}
                {#if column === 'company'}<td><strong>{row.name}</strong></td>
                {:else if column === 'category'}<td class="muted">{row.category ?? '—'}</td>
                {:else if column === 'sources'}<td><div class="chips">{#each (row.sources ?? []) as source}<span class="chip">{sourceLabel(source.source)}</span>{/each}</div></td>
                {:else if column === 'address'}<td class="address">{row.address ?? '—'}</td>
                {:else if column === 'phone'}<td>{row.phones.join(' | ') || '—'}</td>
                {:else if column === 'inn'}<td>{row.inn ?? '—'}</td>
                {:else if column === 'website'}<td>{row.website ?? row.email ?? '—'}</td>
                {:else}<td><div class="chips">{#each (row.tags ?? []).slice(0, 3) as tag}<span class:warn-chip={tag === 'Возможный дубль'} class="chip">{tag}</span>{/each}</div></td>{/if}
              {/each}
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
  table { width: 100%; min-width: 820px; border-collapse: collapse; table-layout: fixed; font-size: 11px; }
  th { position: sticky; top: 0; z-index: 1; background: color-mix(in srgb, var(--panel-solid) 94%, var(--panel-muted)); color: var(--muted); text-align: left; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; font-weight: 700; cursor: grab; user-select: none; }
  th span { margin-left: 3px; opacity: .45; }
  th.dragging { opacity: .45; }
  th, td { padding: 10px 12px; border-bottom: 1px solid var(--line); vertical-align: middle; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .col-company { width: 20%; } .col-category { width: 13%; } .col-sources { width: 13%; } .col-address { width: 20%; } .col-phone { width: 16%; } .col-inn { width: 10%; } .col-website { width: 18%; } .col-tags { width: 16%; }
  tbody tr:hover { background: color-mix(in srgb, var(--fg) 3%, transparent); }
  td.address, td.muted { color: var(--muted); }
  .chips { display:flex; gap:4px; overflow:hidden; }
  .chip { display:inline-flex; height:20px; align-items:center; border:1px solid var(--line); border-radius:999px; padding:0 7px; font-size:9px; background:var(--panel-muted); }
  .warn-chip { color:var(--warning); }
  .empty-row td { text-align: center; color: var(--muted); padding: 48px 16px; }
</style>
