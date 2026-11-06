{{-- Helper mode: each top-level component is server-rendered as a fragment by @stencil. --}}
@extends('layouts.app')

@section('components')
      @stencil<demo-notice kind="info">Rendered by {{ $framework }}.</demo-notice>@endstencil

      @stencil<demo-card heading="Server-rendered card" variant="highlight">
        <span slot="icon">★</span>
        <p>This card was rendered on the server as Declarative Shadow DOM.</p>
        <demo-badge tone="success">SSR</demo-badge>
        <span slot="footer">Footer slot</span>
      </demo-card>@endstencil

      @stencil<demo-counter start="5" step="2"></demo-counter>@endstencil

      @stencil<demo-task-list tasks="{{ json_encode($tasks) }}"></demo-task-list>@endstencil
@endsection
