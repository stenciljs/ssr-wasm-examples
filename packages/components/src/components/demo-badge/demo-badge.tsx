import { Component, Prop, h, Host } from '@stencil/core';

/**
 * Scoped encapsulation: light DOM with scoped CSS classes, styles hoisted into <head>.
 */
@Component({
  tag: 'demo-badge',
  styleUrl: 'demo-badge.css',
  encapsulation: { type: 'scoped' },
})
export class DemoBadge {
  /** Colour tone. */
  @Prop({ reflect: true }) tone: 'neutral' | 'success' | 'warning' = 'neutral';

  render() {
    return (
      <Host>
        <span class="badge">
          <slot />
        </span>
      </Host>
    );
  }
}
