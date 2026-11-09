import { Component, Prop, h } from '@stencil/core';

export interface DemoTask {
  title: string;
  done?: boolean;
}

/**
 * Shadow DOM, a complex (array) prop, and nested Stencil components (`demo-badge`).
 *
 * The ssr-wasm plugin can only receive HTML, so complex data has to arrive as an attribute
 * string. `serialize: 'json'` parses the `tasks` attribute's JSON into the array (and carries the
 * value to the browser in the attribute); on the client the property can also be set directly.
 */
@Component({
  tag: 'demo-task-list',
  styleUrl: 'demo-task-list.css',
  encapsulation: { type: 'shadow' },
})
export class DemoTaskList {
  /** The tasks. As an attribute, a JSON array. */
  @Prop({ serialize: 'json' }) tasks: DemoTask[] = [];

  render() {
    const tasks = Array.isArray(this.tasks) ? this.tasks : [];
    const remaining = tasks.filter((t) => !t.done).length;
    return (
      <section>
        <p class="summary">
          {remaining} of {tasks.length} remaining
        </p>
        <ul>
          {tasks.map((task) => (
            <li class={{ done: !!task.done }}>
              <span class="title">{task.title}</span>
              <demo-badge tone={task.done ? 'success' : 'warning'}>{task.done ? 'done' : 'todo'}</demo-badge>
            </li>
          ))}
        </ul>
      </section>
    );
  }
}
