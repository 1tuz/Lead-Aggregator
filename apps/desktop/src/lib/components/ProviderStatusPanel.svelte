<script lang="ts">
  import { CirclePause, CirclePlay, RotateCcw } from 'lucide-svelte';
  import type { CollectionJobInfo, ProviderStatus, SourceKind } from '../ipc';

  let {
    statuses,
    jobs,
    running,
    onPause,
    onResume,
    onResumeJob,
  }: {
    statuses: ProviderStatus[];
    jobs: CollectionJobInfo[];
    running: boolean;
    onPause: (source: SourceKind) => void;
    onResume: (source: SourceKind) => void;
    onResumeJob: (jobId: string) => void;
  } = $props();

  const labels: Record<SourceKind, string> = {
    twoGis: '2GIS',
    yell: 'Yell',
    zoon: 'Zoon',
    rusprofile: 'Rusprofile',
  };

  const stateLabels: Record<ProviderStatus['state'], string> = {
    queued: 'В очереди',
    running: 'Сбор',
    paused: 'Пауза',
    rateLimited: 'HTTP 429',
    blocked: 'HTTP 403',
    captchaRequired: 'CAPTCHA',
    challengeRequired: 'Anti-bot',
    completed: 'Готово',
    failed: 'Ошибка',
    cancelled: 'Остановлено',
  };

  const resumableJobs = $derived(jobs.filter((job) => ['interrupted', 'partial', 'failed', 'cancelled'].includes(job.state)));
</script>

{#if statuses.length > 0 || resumableJobs.length > 0}
  <section class="panel">
    {#if statuses.length > 0}
      <div class="status-grid">
        {#each statuses as status (status.source)}
          <article class:warning={['rateLimited', 'blocked', 'captchaRequired', 'challengeRequired', 'failed'].includes(status.state)}>
            <div class="title"><strong>{labels[status.source]}</strong><span>{stateLabels[status.state]}</span></div>
            <p>{status.region || '—'} · {status.message}</p>
            {#if status.total && status.total > 0}
              <div class="bar"><i style={`width:${Math.min(100, Math.round((status.current / status.total) * 100))}%`}></i></div>
              <small>{status.current.toLocaleString()} / {status.total.toLocaleString()}</small>
            {/if}
            {#if status.retryAfterSeconds}
              <small>Повтор не раньше чем через {status.retryAfterSeconds} сек.</small>
            {/if}
            {#if running && status.state === 'paused'}
              <button onclick={() => onResume(status.source)}><CirclePlay size={13} /> Продолжить</button>
            {:else if running && status.state === 'running'}
              <button onclick={() => onPause(status.source)}><CirclePause size={13} /> Пауза</button>
            {/if}
          </article>
        {/each}
      </div>
    {/if}

    {#if resumableJobs.length > 0 && !running}
      <div class="resume-list">
        <strong>Незавершённые задания</strong>
        {#each resumableJobs.slice(0, 4) as job (job.jobId)}
          <div class="resume-row">
            <span><b>{job.request.query}</b><small>{job.completedTargets} / {job.totalTargets} целей · {job.message}</small></span>
            <button onclick={() => onResumeJob(job.jobId)}><RotateCcw size={13} /> Продолжить</button>
          </div>
        {/each}
      </div>
    {/if}
  </section>
{/if}

<style>
  .panel { margin:12px 18px 0; display:grid; gap:9px; }
  .status-grid { display:grid; grid-template-columns:repeat(4, minmax(0, 1fr)); gap:7px; }
  article { min-width:0; border:1px solid var(--line); border-radius:10px; background:var(--panel-solid); padding:9px; }
  article.warning { border-color:color-mix(in srgb, var(--warning) 35%, var(--line)); }
  .title { display:flex; justify-content:space-between; gap:8px; font-size:10px; }
  .title strong { font-size:11px; }
  .title span { color:var(--muted); }
  p { margin:7px 0; color:var(--muted); font-size:10px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  small { display:block; margin-top:5px; color:var(--muted); font-size:9px; }
  .bar { height:3px; border-radius:99px; overflow:hidden; background:var(--panel-muted); }
  .bar i { display:block; height:100%; background:var(--accent); }
  button { margin-top:8px; height:28px; border:1px solid var(--line); border-radius:7px; background:var(--panel-muted); color:var(--fg); padding:0 8px; display:inline-flex; align-items:center; gap:5px; font-size:10px; }
  .resume-list { border:1px solid var(--line); border-radius:10px; background:var(--panel-solid); padding:9px; display:grid; gap:7px; }
  .resume-list > strong { font-size:10px; text-transform:uppercase; letter-spacing:.05em; color:var(--muted); }
  .resume-row { display:flex; justify-content:space-between; align-items:center; gap:8px; }
  .resume-row span { min-width:0; font-size:10px; }
  .resume-row b { display:block; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .resume-row button { margin:0; flex:0 0 auto; }
  @media (max-width: 1050px) { .status-grid { grid-template-columns:repeat(2, 1fr); } }
</style>
