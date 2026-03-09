<script lang="ts">
  import { api, type IpList } from './api';
  import ListManager from './ListManager.svelte';

  const initialNewEntity: IpList = {
    url: '',
    update_interval_seconds: 86400,
    last_updated: null,
    interface: null
  };
</script>

<ListManager
  title="IP list"
  addLabel="Add IP list"
  emptyMessage="No IP lists configured."
  fetchData={api.getIpLists}
  addEntity={api.addIpList}
  removeEntity={api.removeIpList}
  syncEntity={api.syncIpList}
  {initialNewEntity}
/>
