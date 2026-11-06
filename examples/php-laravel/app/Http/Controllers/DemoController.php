<?php

namespace App\Http\Controllers;

use Illuminate\View\View;

class DemoController
{
    /** Deliberately includes <, >, & and quotes, to check the template engine's escaping. */
    private const TASKS = [
        ['title' => 'Write components', 'done' => true],
        ['title' => 'Render with <wasm> & "friends"', 'done' => false],
        ['title' => 'Hydrate on the client'],
    ];

    public function middleware(): View
    {
        return view('middleware', ['framework' => 'Laravel', 'tasks' => self::TASKS]);
    }

    public function helper(): View
    {
        return view('helper', ['framework' => 'Laravel', 'tasks' => self::TASKS]);
    }
}
