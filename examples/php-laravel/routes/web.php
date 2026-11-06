<?php

use App\Http\Controllers\DemoController;
use App\Http\Controllers\StencilAssetController;
use App\Http\Middleware\RenderStencilComponents;
use Illuminate\Support\Facades\Route;

// Middleware mode: the view renders plain component tags; the middleware renders the whole document.
Route::get('/', [DemoController::class, 'middleware'])->middleware(RenderStencilComponents::class);

// Helper mode: the view wraps each component in @stencil ... @endstencil. No post-processing.
Route::get('/helper', [DemoController::class, 'helper']);

Route::get('/health', fn () => response('ok', 200, ['Content-Type' => 'text/plain; charset=utf-8']));

// The Stencil client bundle (STENCIL_CLIENT_DIR).
Route::get('/stencil/{path}', StencilAssetController::class)->where('path', '.*');
