<script lang="ts">
  import { toast } from './toast_state.svelte';
  import { flip } from 'svelte/animate';
  import { fade, fly } from 'svelte/transition';
</script>

<div class="fixed bottom-4 right-4 z-50 flex flex-col gap-2 pointer-events-none">
  {#each toast.toasts as t (t.id)}
    <div
      in:fly={{ x: 100, duration: 300 }}
      out:fade={{ duration: 200 }}
      class="pointer-events-auto px-6 py-3 border border-zinc-700 bg-zinc-900 text-sm font-bold min-w-[200px]
             {t.type === 'success' ? 'border-l-4 border-l-green-500' : ''}
             {t.type === 'error' ? 'border-l-4 border-l-red-500' : ''}
             {t.type === 'info' ? 'border-l-4 border-l-blue-500' : ''}"
    >
      {t.message}
    </div>
  {/each}
</div>
