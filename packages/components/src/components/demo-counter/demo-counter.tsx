import { Component, Event, EventEmitter, Prop, State, h } from '@stencil/core';

/**
 * Shadow DOM + @State + componentWillLoad + an event. Used to prove the server-rendered
 * markup hydrates into a live, interactive component on the client.
 */
@Component({
  tag: 'demo-counter',
  styleUrl: 'demo-counter.css',
  encapsulation: { type: 'shadow' },
})
export class DemoCounter {
  /** Starting value. */
  @Prop() start = 0;

  /** Amount added per click. */
  @Prop() step = 1;

  @State() count: number;

  /** Emitted with the new count after each click. */
  @Event() countChanged: EventEmitter<number>;

  componentWillLoad() {
    this.count = this.start;
  }

  private increment = () => {
    this.count += this.step;
    this.countChanged.emit(this.count);
  };

  render() {
    return (
      <div class="counter">
        <span class="value" part="value">
          {this.count}
        </span>
        <button type="button" onClick={this.increment} part="button">
          +{this.step}
        </button>
      </div>
    );
  }
}
