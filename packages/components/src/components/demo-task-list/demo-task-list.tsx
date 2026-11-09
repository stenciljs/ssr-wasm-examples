import { Component, Prop, h } from '@stencil/core';

export interface DemoTask {
  title: string;
  done?: boolean;
}

/**
 * Shadow DOM, an array prop, and nested Stencil components (`demo-badge`).
 *
 * The server can only pass HTML to the plugin, so the array arrives as a JSON attribute:
 * `serialize: 'json'` parses it into `tasks`. In the browser the property can also be set
 * directly.
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
