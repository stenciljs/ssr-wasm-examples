import { Component, Prop, h, Host } from '@stencil/core';

/**
 * No encapsulation: plain light DOM, styled by the page's global CSS.
 */
@Component({
  tag: 'demo-notice',
})
export class DemoNotice {
  /** Notice kind - becomes a class the host page can style. */
  @Prop() kind: 'info' | 'error' = 'info';

  render() {
    return (
      <Host class={`demo-notice demo-notice--${this.kind}`} role="status">
        <strong class="demo-notice__label">{this.kind === 'error' ? 'Error' : 'Note'}:</strong>{' '}
        <slot />
      </Host>
    );
  }
}
