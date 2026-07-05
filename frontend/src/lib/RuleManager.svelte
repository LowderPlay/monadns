<script lang="ts" generics="T extends { interface: string | null; }">
import type { Snippet } from "svelte";
import { onMount } from "svelte";
import InterfaceSelect from "./InterfaceSelect.svelte";
import { toast } from "./toast_state.svelte";

interface Props {
	title: string;
	addLabel: string;
	emptyMessage: string;
	fetchData: () => Promise<T[]>;
	addEntity: (entity: T) => Promise<any>;
	removeEntity: (id: any) => Promise<any>;
	initialNewEntity: T;
	idField: keyof T;
	idLabel: string;
	// Snippets for customization
	extraFields?: Snippet<[{ entity: T }]>;
	extraHeaders?: Snippet;
	extraCells?: Snippet<[{ entity: T }]>;
}

let {
	title,
	addLabel,
	emptyMessage,
	fetchData,
	addEntity,
	removeEntity,
	initialNewEntity,
	idField,
	idLabel,
	extraFields,
	extraHeaders,
	extraCells,
}: Props = $props();

let items = $state<T[]>([]);
let error = $state<string | null>(null);
let loading = $state(true);
let savingInterfaces = $state<Record<string, boolean>>({});

// Form for adding new rule
let newEntity = $state<T>({ ...initialNewEntity });
let adding = $state(false);

async function loadData() {
	loading = true;
	try {
		items = await fetchData();
	} catch (e: any) {
		error = e.message;
		toast.error("Failed to load data: " + e.message);
	} finally {
		loading = false;
	}
}

onMount(loadData);

async function addItem() {
	if (!newEntity[idField]) return;
	adding = true;
	try {
		await addEntity(newEntity);
		newEntity = { ...initialNewEntity };
		toast.success(`${title} added`);
		await loadData();
	} catch (e: any) {
		error = e.message;
		toast.error(`Failed to add ${title.toLowerCase()}: ` + e.message);
	} finally {
		adding = false;
	}
}

async function deleteItem(id: any) {
	if (!confirm(`Remove ${id}?`)) return;
	try {
		await removeEntity(id);
		toast.success(`${title} removed`);
		await loadData();
	} catch (e: any) {
		error = e.message;
		toast.error(`Failed to remove ${title.toLowerCase()}: ` + e.message);
	}
}

function itemKey(item: T) {
	return String(item[idField] ?? "");
}

function interfaceSelectId(item: T) {
	return `interface_${itemKey(item).replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

async function updateInterface(item: T, value: string | null) {
	const key = itemKey(item);
	if (!key || item.interface === value || savingInterfaces[key]) return;

	const previousInterface = item.interface;
	const updated = { ...item, interface: value } as T;
	items = items.map((current) =>
		itemKey(current) === key ? updated : current,
	);
	savingInterfaces = { ...savingInterfaces, [key]: true };

	try {
		await addEntity(updated);
		toast.success(`${title} interface updated`);
	} catch (e: any) {
		items = items.map((current) =>
			itemKey(current) === key
				? ({ ...current, interface: previousInterface } as T)
				: current,
		);
		error = e.message;
		toast.error(
			`Failed to update ${title.toLowerCase()} interface: ` + e.message,
		);
	} finally {
		const nextSavingInterfaces = { ...savingInterfaces };
		delete nextSavingInterfaces[key];
		savingInterfaces = nextSavingInterfaces;
	}
}
</script>

<div class="space-y-6">
  <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">{title}s</h2>

  {#if error}
    <div class="bg-red-900/20 text-red-400 p-4 border border-red-800">
      {error}
      <button onclick={() => error = null} class="ml-2 underline text-sm">Dismiss</button>
    </div>
  {/if}

  <!-- Add New Rule Form -->
  <div class="bg-zinc-900 p-4 border border-zinc-800 space-y-4">
    <h3 class="text-lg font-bold">Add new {title.toLowerCase()} rule</h3>
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4 items-end">
      <div class="flex flex-col gap-1 lg:col-span-2">
        <label for="rule_entity" class="text-sm text-zinc-400 font-bold">{title}</label>
        <input id="rule_entity" bind:value={newEntity[idField]} placeholder="Enter {title.toLowerCase()}..." class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500 h-10" />
      </div>

      <InterfaceSelect 
        value={newEntity.interface} 
        onchange={(val) => { newEntity.interface = val; }}
      />
      {#if extraFields}
        {@render extraFields({ entity: newEntity })}
      {/if}

      <button onclick={addItem} disabled={adding} class="bg-white text-black px-4 py-2 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors uppercase text-xs tracking-widest h-10">
        {adding ? 'Adding...' : addLabel}
      </button>
    </div>
  </div>


  <!-- Rules Table -->
  <div class="overflow-x-auto">
    <table class="w-full border-collapse text-left">
      <thead>
        <tr class="border-b border-zinc-800">
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">{idLabel}</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Interface</th>
          
          {#if extraHeaders}
            {@render extraHeaders()}
          {/if}

          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-right">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each items as item}
          <tr class="border-b border-zinc-900 hover:bg-zinc-900/50">
            <td class="p-2 tracking-tight font-mono">{item[idField]}</td>
            <td class="p-2 text-center text-sm font-mono text-zinc-400 min-w-40">
              <InterfaceSelect
                id={interfaceSelectId(item)}
                value={item.interface}
                onchange={(val) => updateInterface(item, val)}
                showLabel={false}
                compact
                disabled={savingInterfaces[itemKey(item)]}
              />
            </td>
            
            {#if extraCells}
              {@render extraCells({ entity: item })}
            {/if}

            <td class="p-2 text-right">
              <button onclick={() => deleteItem(item[idField])} class="text-red-500 hover:text-red-400 font-bold text-xs uppercase tracking-widest transition-colors">Delete</button>
            </td>
          </tr>
        {/each}
        {#if items.length === 0 && !loading}
          <tr>
            <td colspan="4" class="p-12 text-center text-zinc-600 italic tracking-wide">{emptyMessage}</td>
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
</div>
