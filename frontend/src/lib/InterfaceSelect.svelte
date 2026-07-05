<script lang="ts">
import { onMount } from "svelte";
import { api, type Config } from "./api";

interface Props {
	value: string | null;
	onchange: (value: string | null) => void | Promise<void>;
	id?: string;
	showLabel?: boolean;
	compact?: boolean;
	disabled?: boolean;
}

let {
	value,
	onchange,
	id = "interface_select",
	showLabel = true,
	compact = false,
	disabled = false,
}: Props = $props();
let config = $state<Config | null>(null);

onMount(async () => {
	try {
		config = await api.getConfig();
	} catch (e) {
		console.error("Failed to load config for InterfaceSelect", e);
	}
});

function handleChange(e: Event) {
	const val = (e.target as HTMLSelectElement).value;
	void onchange(val === "default" ? null : val);
}
</script>

<div class="flex flex-col gap-1">
  {#if showLabel}
    <label for={id} class="text-sm text-zinc-400 font-bold">Interface</label>
  {/if}
  <select 
    {id} 
    value={value === null ? 'default' : value} 
    onchange={handleChange}
    {disabled}
    class="bg-zinc-950 border border-zinc-700 focus:outline-none focus:border-zinc-500 w-full disabled:opacity-60 disabled:cursor-wait {compact ? 'px-2 py-1 text-xs' : 'p-2'}"
  >
    <option value="default">Default ({config?.default_interface || '...' })</option>
    {#if config}
      {#each config.interfaces as iface}
        <option value={iface.name}>{iface.name}</option>
      {/each}
    {/if}
  </select>
</div>
