<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { engineCall, listJobs, readable, type JobList } from '../engineTools';

const props = defineProps<{ root: string }>();
const result = ref<JobList>(), error = ref(''), loading = ref(false);
async function refresh() {
  loading.value = true;
  error.value = '';
  try { result.value = await listJobs(props.root); }
  catch (cause) { result.value = undefined; error.value = readable(cause); }
  finally { loading.value = false; }
}
const stopping = ref('');
const ended = (status: string) => /exit|done|stop|finish|fail|kill|complete/i.test(status);
async function stop(name: string) {
  if (stopping.value) return;
  stopping.value = name;
  try { await engineCall('stop_job', { name }, props.root); }
  catch (cause) { error.value = readable(cause); }
  finally { stopping.value = ''; }
  if (!error.value) await refresh();
}
onMounted(refresh);
</script>
<template>
  <section aria-label="Background jobs">
    <div class="head"><strong>Background jobs</strong><button class="btn btn-secondary" :disabled="loading" @click="refresh">{{ loading ? 'Loading…' : 'Refresh' }}</button></div>
    <p v-if="error" role="alert">{{ error }}</p>
    <template v-if="result">
      <p v-for="job in result.jobs" :key="'session:' + job.name"><code>{{ job.name }}</code> · Session · {{ job.status }}<button v-if="!ended(job.status)" class="s-link danger stop" :disabled="!!stopping" @click="stop(job.name)">{{ stopping === job.name ? 'Stopping…' : 'Stop' }}</button></p>
      <p v-for="job in result.persistent_jobs" :key="'persistent:' + job.name"><code>{{ job.name }}</code> · Persistent (read only) · {{ job.status }}<template v-if="job.lifetime_secs != null"> · Lifetime {{ job.lifetime_secs }} s</template></p>
      <p v-if="!result.jobs.length && !result.persistent_jobs?.length">{{ result.persistent_jobs ? 'No background jobs found.' : 'No session jobs found.' }}</p>
      <p v-if="!result.persistent_jobs">Persistent discovery unavailable{{ result.persistent_error ? ': ' + result.persistent_error : ' on this engine.' }}</p>
    </template>
  </section>
</template>
<style scoped>
section { border-bottom: 1px solid var(--border); padding-bottom: 10px; font-size: var(--text-sm); overflow-wrap: anywhere; }
.head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
p { color: var(--text-muted); margin: 6px 0; }
.stop { margin-left: 8px; }
</style>
