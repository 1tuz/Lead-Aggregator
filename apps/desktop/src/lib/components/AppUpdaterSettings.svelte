<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { Check, Download, RefreshCw } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../ipc';

  let { currentVersion = '' }: { currentVersion?: string } = $props();
  let autoCheck = $state(true);
  let checking = $state(false);
  let installing = $state(false);
  let updateVersion = $state<string | null>(null);
  let notes = $state<string | null>(null);
  let status = $state('Проверка обновлений включена');
  let error = $state('');
  let downloaded = $state(0);
  let total = $state<number | null>(null);

  const progress = $derived(total && total > 0 ? Math.min(100, Math.round(downloaded / total * 100)) : null);

  async function check(silent = false) {
    if (checking || installing) return;
    checking = true;
    error = '';
    try {
      const result = await api.checkForUpdates();
      updateVersion = result.version;
      notes = result.notes;
      if (!result.version) status = silent ? 'Приложение обновлено' : 'Установлена последняя версия';
      else status = `Доступна версия ${result.version}`;
    } catch (cause) {
      if (!silent) error = errorMessage(cause);
      status = silent ? 'Не удалось проверить обновления' : '';
    } finally {
      checking = false;
    }
  }

  async function install() {
    if (!updateVersion || installing) return;
    installing = true;
    error = '';
    downloaded = 0;
    total = null;
    status = 'Скачивание и установка…';
    try {
      await api.installUpdate();
    } catch (cause) {
      error = errorMessage(cause);
      installing = false;
      status = '';
    }
  }

  onMount(() => {
    autoCheck = localStorage.getItem('auto-update-check') !== 'false';
    const unlisten = listen<{ downloaded: number; total: number | null }>('update-progress', ({ payload }) => {
      downloaded = payload.downloaded;
      total = payload.total;
    });
    const initial = window.setTimeout(() => { if (autoCheck) void check(true); }, 1200);
    const interval = window.setInterval(() => { if (autoCheck) void check(true); }, 6 * 60 * 60 * 1000);
    return () => {
      window.clearTimeout(initial);
      window.clearInterval(interval);
      void unlisten.then((stop) => stop());
    };
  });

  function toggleAutoCheck(event: Event) {
    autoCheck = (event.currentTarget as HTMLInputElement).checked;
    localStorage.setItem('auto-update-check', String(autoCheck));
    if (autoCheck) void check(true);
  }
</script>

<section class="update-card" aria-label="Обновления приложения">
  <div class="update-heading"><span class="update-icon"><RefreshCw size={14} /></span><div><strong>Обновления приложения</strong><small>Установлена версия {currentVersion || '—'}</small></div></div>
  <label class="auto-check"><input type="checkbox" checked={autoCheck} onchange={toggleAutoCheck} /><span>Автоматически проверять обновления</span></label>
  <p class="update-status" aria-live="polite">{status}</p>
  {#if notes && updateVersion}<p class="update-notes">{notes}</p>{/if}
  {#if progress !== null}<progress max="100" value={progress}></progress>{:else if installing}<progress></progress>{/if}
  {#if error}<p class="update-error">{error}</p>{/if}
  <div class="update-actions">
    <button type="button" onclick={() => void check()} disabled={checking || installing} aria-label="Проверить обновления"><RefreshCw size={13} /> {checking ? 'Проверяю…' : 'Проверить'}</button>
    {#if updateVersion}<button type="button" class="install" onclick={() => void install()} disabled={installing}><Download size={13} /> {installing ? 'Обновляю…' : `Обновить до ${updateVersion}`}</button>{/if}
  </div>
</section>

<style>
  .update-card { display:grid; gap:8px; border:1px solid var(--line); background:var(--panel-muted); border-radius:11px; padding:10px; }
  .update-heading { display:flex; align-items:center; gap:8px; }
  .update-heading strong { display:block; font-size:11px; }
  .update-heading small { display:block; color:var(--muted); font-size:9px; margin-top:2px; }
  .update-icon { display:grid; place-items:center; width:25px; height:25px; border:1px solid var(--line); border-radius:8px; background:var(--panel-solid); }
  .auto-check { display:flex; align-items:center; gap:7px; color:var(--fg); font-size:10px; cursor:pointer; }
  .auto-check input { width:14px; height:14px; margin:0; accent-color:var(--fg); }
  .update-status,.update-error,.update-notes { margin:0; font-size:10px; line-height:1.4; color:var(--muted); overflow-wrap:anywhere; }
  .update-error { color:var(--danger); }
  progress { width:100%; height:5px; accent-color:var(--fg); }
  .update-actions { display:flex; flex-wrap:wrap; gap:6px; }
  .update-actions button { min-height:29px; border:1px solid var(--line); border-radius:8px; background:var(--panel-solid); color:var(--fg); padding:0 8px; display:flex; align-items:center; justify-content:center; gap:5px; font-size:10px; font-weight:600; }
  .update-actions button.install { flex:1; background:var(--fg); color:var(--panel-solid); }
  .update-actions button:disabled { opacity:.5; }
</style>
