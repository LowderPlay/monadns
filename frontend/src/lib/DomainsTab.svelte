<script lang="ts">
import { api, type DomainRule } from "./api";
import RuleManager from "./RuleManager.svelte";

const initialNewEntity: DomainRule = {
	domain: "",
	include_subdomains: true,
	interface: null,
};
</script>

<RuleManager
  title="Domain rule"
  addLabel="Add domain"
  emptyMessage="No custom domain rules configured."
  fetchData={api.getDomains}
  addEntity={api.addDomain}
  removeEntity={api.removeDomain}
  {initialNewEntity}
  idField="domain"
  idLabel="Domain"
>
  {#snippet extraFields({ entity })}
    <div class="flex items-center gap-2 h-10">
      <input type="checkbox" id="sub_domain" bind:checked={entity.include_subdomains} class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white" />
      <label for="sub_domain" class="text-sm text-zinc-400 font-bold">Subdomains?</label>
    </div>
  {/snippet}

  {#snippet extraHeaders()}
    <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Subdomains</th>
  {/snippet}

  {#snippet extraCells({ entity })}
    <td class="p-2 text-center text-sm font-mono">{entity.include_subdomains ? 'YES' : 'NO'}</td>
  {/snippet}
</RuleManager>
