<!-- Each element below is one error svelte-check must report (scripts/kit-types.test.ts counts them): a wrong attribute or
     event name, or a wrong value, is flagged in the editor and the build. -->
<script lang="ts"></script>

<ui-button varient="tinted" />
<ui-button variant="tinteed" />
<ui-toggle onchange={(e) => e.detail.value} />
<ui-toggle checked="yes" />
