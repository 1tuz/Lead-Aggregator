<script lang="ts">
  import { KeyRound, LoaderCircle, Trash2 } from 'lucide-svelte';

  let {
    saved,
    disabled = false,
    onSave,
    onDelete,
  }: {
    saved: boolean;
    disabled?: boolean;
    onSave: (key: string) => Promise<void>;
    onDelete: () => Promise<void>;
  } = $props();

  let key = $state('');
  let busy = $state(false);
  let error = $state('');

  async function save() {
    if (!key.trim() || busy || disabled) return;
    busy = true;
    error = '';
    try {
      await onSave(key.trim());
      key = '';
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Не удалось сохранить ключ';
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (busy || disabled) return;
    busy = true;
    error = '';
    try {
      await onDelete();
      key = '';
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Не удалось удалить ключ';
    } finally {
      busy = false;
    }
  }
</script>

<section class="api-key-card" aria-label="Лицензионный ключ">
  <div class="card-heading">
    <span class="icon"><KeyRound size={14} /></span>
    <div class="heading-copy">
      <strong>Лицензионный ключ</strong>
      <small>{saved ? 'Ключ сохранён и проверен сервисом' : 'Для сбора нужен лицензионный ключ'}</small>
    </div>
    <span class:saved class="state-dot" aria-label={saved ? 'Ключ сохранён' : 'Ключ не сохранён'}></span>
  </div>

  <label class="key-label" for="twogis-api-key">Ключ API</label>
  <div class="key-row">
    <input
      id="parselab-key"
      type="password"
      bind:value={key}
      placeholder={saved ? 'Вставь новый ключ для замены' : 'Вставь лицензионный ключ'}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      disabled={disabled || busy}
      onkeydown={(event) => event.key === 'Enter' && save()}
      aria-label="Лицензионный ключ"
    />
    {#if saved}
      <button class="icon-button" type="button" onclick={remove} disabled={disabled || busy} aria-label="Удалить сохранённый ключ" title="Удалить ключ">
        <Trash2 size={14} />
      </button>
    {/if}
  </div>
  <div class="footer-row">
    <small>Ключ проверяется сервисом перед сохранением и используется для скачивания выгрузок.</small>
    <button class="save-button" type="button" onclick={save} disabled={disabled || busy || !key.trim()}>
      {#if busy}<LoaderCircle class="spin" size={13} />{:else}Сохранить{/if}
    </button>
  </div>
  {#if saved}<p class="privacy-note">Ключ хранится в системном хранилище и не добавляется в историю поиска.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .api-key-card { display:grid; gap:8px; border:1px solid var(--line); border-radius:10px; background:var(--panel-muted); padding:10px; }
  .card-heading { display:flex; align-items:center; gap:8px; min-width:0; }
  .icon { display:grid; place-items:center; width:25px; height:25px; flex:0 0 25px; border:1px solid var(--line); border-radius:8px; color:var(--fg); background:var(--panel-solid); }
  .heading-copy { display:grid; gap:2px; min-width:0; }
  .heading-copy strong { color:var(--fg); font-size:11px; font-weight:650; }
  .heading-copy small,.footer-row small,.privacy-note { color:var(--muted); font-size:9px; line-height:1.4; }
  .state-dot { width:7px; height:7px; margin-left:auto; flex:0 0 7px; border-radius:50%; background:var(--muted); }
  .state-dot.saved { background:#3b9b69; }
  .key-label { margin-top:2px; color:var(--muted); font-size:9px; font-weight:650; text-transform:uppercase; letter-spacing:.045em; }
  .key-row { display:flex; gap:6px; }
  input { width:100%; min-width:0; height:32px; border:1px solid var(--line); border-radius:8px; background:var(--panel-solid); color:var(--fg); padding:0 9px; font:inherit; font-size:10px; }
  input:focus-visible,button:focus-visible { outline:2px solid color-mix(in srgb,var(--fg) 45%,transparent); outline-offset:2px; }
  .icon-button,.save-button { display:inline-flex; justify-content:center; align-items:center; gap:5px; flex:0 0 auto; height:30px; border:1px solid var(--line); border-radius:8px; background:var(--panel-solid); color:var(--fg); cursor:pointer; }
  .icon-button { width:32px; color:var(--muted); }
  .save-button { padding:0 9px; font-size:10px; font-weight:600; }
  button:disabled { opacity:.45; cursor:default; }
  .footer-row { display:flex; align-items:center; justify-content:space-between; gap:8px; }
  .footer-row small { max-width:190px; }
  .privacy-note { margin:0; }
  .error { margin:0; color:var(--danger, #b84343); font-size:10px; }
  :global(.spin) { animation:spin .9s linear infinite; }
  @keyframes spin { to { transform:rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { :global(.spin) { animation:none; } }
</style>
