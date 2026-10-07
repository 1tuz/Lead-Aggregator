<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { Download, RefreshCw } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../ipc';

  let { currentVersion = '' }: { currentVersion?: string } = $props();
  let checking = $state(false);
  let installing = $state(false);
  let updateVersion = $state<string | null>(null);
  let status = $state('Проверка…');
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
      status = result.version ? `Доступна версия ${result.version}` : 'Установлена последняя версия';
    } catch (cause) {
      if (!silent) error = errorMessage(cause);
      status = 'Не удалось проверить обновления';
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
      status = 'Не удалось установить обновление';
    }
  }

  onMount(() => {
    const unlisten = listen<{ downloaded: number; total: number | null }>('update-progress', ({ payload }) => {
      downloaded = payload.downloaded;
      total = payload.total;
    });
    void check(true);
    const interval = window.setInterval(() => void check(true), 6 * 60 * 60 * 1000);
    return () => {
      window.clearInterval(interval);
      void unlisten.then((stop) => stop());
    };
  });
</script>

<section class="update-row" aria-label="Обновление приложения">
  <div class="update-copy">
    <strong>Обновление приложения</strong>
    <small>Версия {currentVersion || '—'} · {status}</small>
    {#if error}<small class="error">{error}</small>{/if}
    {#if progress !== null}<progress max="100" value={progress}></progress>{:else if installing}<progress></progress>{/if}
  </div>
  {#if updateVersion}
    <button type="button" class="install" onclick={() => void install()} disabled={installing} aria-label={`Установить версию ${updateVersion}`} title={`Установить ${updateVersion}`}><Download size={14} /></button>
  {:else}
    <button type="button" onclick={() => void check()} disabled={checking || installing} aria-label="Проверить обновления" title="Проверить обновления"><RefreshCw size={14} /></button>
  {/if}
</section>

<style>
  .update-row { display:flex; align-items:center; gap:7px; min-width:0; flex:1; }
  .update-copy { display:grid; gap:3px; min-width:0; flex:1; }
  strong { color:var(--fg); font-size:10px; font-weight:650; }
  small { color:var(--muted); font-size:9px; line-height:1.35; overflow-wrap:anywhere; }
  small.error { color:var(--danger); }
  progress { width:100%; height:3px; accent-color:var(--fg); }
  button { display:grid; place-items:center; flex:0 0 30px; width:30px; height:30px; border:1px solid var(--line); border-radius:8px; background:var(--panel-solid); color:var(--fg); cursor:pointer; }
  button.install { border-color:transparent; background:var(--accent); color:var(--accent-fg); }
  button:disabled { opacity:.5; cursor:default; }
</style>
