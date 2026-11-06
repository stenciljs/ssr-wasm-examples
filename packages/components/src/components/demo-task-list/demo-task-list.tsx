import { AttrDeserialize, Component, Prop, h } from '@stencil/core';

export interface DemoTask {
  title: string;
  done?: boolean;
}

/**
 * Shadow DOM, a complex (array) prop, and nested Stencil components (`demo-badge`).
 *
 * The ssr-wasm plugin can only receive HTML, so complex data has to arrive as an attribute
 * string. `@AttrDeserialize` turns the `tasks` attribute's JSON into the array; on the client
 * the property can also be set directly.
 */
@Component({
  tag: 'demo-task-list',
  styleUrl: 'demo-task-list.css',
  encapsulation: { type: 'shadow' },
})
export class DemoTaskList {
  /** The tasks. As an attribute, a JSON array. */
  @Prop() tasks: DemoTask[] = [];

  @AttrDeserialize('tasks')
  deserializeTasks(value: string): DemoTask[] {
    try {
      const json = JSON.parse(value);
      return Array.isArray(json) ? json : [];
    } catch {
      return [];
    }
  }

  render() {
    const tasks = this.tasks ?? [];
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
