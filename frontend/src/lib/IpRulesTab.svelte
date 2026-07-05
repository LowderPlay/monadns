<script lang="ts">
import { api, type IpRule } from "./api";
import RuleManager from "./RuleManager.svelte";

const initialNewEntity: IpRule = {
	subnet: "",
	interface: null,
};
</script>

<RuleManager
  title="IP Rule"
  addLabel="Add IP Rule"
  emptyMessage="No custom IP rules configured."
  fetchData={api.getIps}
  addEntity={api.addIp}
  removeEntity={api.removeIp}
  {initialNewEntity}
  idField="subnet"
  idLabel="Subnet (CIDR)"
/>
