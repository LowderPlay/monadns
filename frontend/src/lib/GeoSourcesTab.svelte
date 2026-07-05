<script lang="ts">
import { api, type GeoSource } from "./api";
import ListManager from "./ListManager.svelte";

const initialNewEntity: GeoSource = {
	url: "",
	type: "geosite",
	update_interval_seconds: 86400,
	last_updated: null,
};
</script>

<ListManager
  title="Geo source"
  addLabel="Add source"
  emptyMessage="No geo sources configured."
  fetchData={api.getGeoSources}
  addEntity={api.addGeoSource}
  removeEntity={api.removeGeoSource}
  syncEntity={api.syncGeoSource}
  {initialNewEntity}
  showInterface={false}
>
  {#snippet extraFields({ entity })}
    <div class="flex flex-col gap-1">
      <label for="source_type" class="text-sm text-zinc-400 font-bold">Type</label>
      <select id="source_type" bind:value={entity.type} class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500">
        <option value="geosite">GeoSite</option>
        <option value="geoip">GeoIP</option>
      </select>
    </div>
  {/snippet}

  {#snippet extraHeaders()}
    <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Type</th>
  {/snippet}

  {#snippet extraCells({ entity })}
    <td class="p-2 text-center text-sm font-mono uppercase text-zinc-400">{entity.type}</td>
  {/snippet}
</ListManager>
