<script setup lang="ts">
import { EllipsisVertical, Hash, KeyRound, Link2, Pen, Phone, Plus, Search } from '@lucide/vue';
import Input from './Input.vue';
import Button from './Button.vue';
</script>

<template>
  <article>
    <h2>Inputs:</h2>

    <section id="inputs">
      <Input :icon-component="Search" type="text" placeholder="e.g., CoolUser233" />

      <Input :icon-component="KeyRound" type="password" placeholder="Strong password..." />

      <Input :icon-component="Hash" type="number" placeholder="Number..." />

      <Input :icon-component="Phone" type="tel" placeholder="01234 567 890..." />

      <Input disabled type="text" placeholder="Disabled" />

      <Input :icon-component="Link2" type="url" placeholder="google.com" />
    </section>

    <h2>Buttons:</h2>

    <section id="buttons">
      <Button variant="accent" :icon-component="Plus">Create</Button>
      <Button variant="accent" :icon-component="Plus" icon-only>Create</Button>
      <Button variant="accent" size="small" :icon-component="Plus">Create</Button>
      <Button variant="accent" size="small" :icon-component="Plus" icon-only>Create</Button>

      <Button variant="outline" :icon-component="Pen">Edit</Button>
      <Button variant="outline" :icon-component="Pen" icon-only>Edit</Button>
      <Button variant="outline" size="small" :icon-component="Pen">Edit</Button>
      <Button variant="outline" size="small" :icon-component="Pen" icon-only>Edit</Button>

      <Button variant="label" :icon-component="EllipsisVertical">Menu</Button>
      <Button variant="label" :icon-component="EllipsisVertical" icon-only>Menu</Button>
      <Button variant="label" size="small" :icon-component="EllipsisVertical">Menu</Button>
      <Button variant="label" size="small" :icon-component="EllipsisVertical" icon-only>
        Menu
      </Button>
    </section>
  </article>
</template>

<style scoped>
article {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

#inputs {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1rem;
}

#buttons {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  align-items: center;
  gap: 1rem;

  button {
    margin: auto;
  }
}
</style>
