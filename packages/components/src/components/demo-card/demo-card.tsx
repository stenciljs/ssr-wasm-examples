import { Component, Prop, h } from '@stencil/core';

/**
 * Shadow DOM, named + default slots, and a `::part` for theming from outside.
 * Rendered by the server as Declarative Shadow DOM (`<template shadowrootmode="open">`).
 */
@Component({
  tag: 'demo-card',
  styleUrl: 'demo-card.css',
  encapsulation: { type: 'shadow' },
})
export class DemoCard {
  /** Card heading. */
  @Prop() heading: string;

  /** Visual variant. */
  @Prop({ reflect: true }) variant: 'default' | 'highlight' = 'default';

  render() {
    return (
      <article class="card" part="container">
        <header part="header">
          <slot name="icon" />
          <h2>{this.heading}</h2>
        </header>
        <div class="body">
          <slot />
        </div>
        <footer>
          <slot name="footer" />
        </footer>
      </article>
    );
  }
}
