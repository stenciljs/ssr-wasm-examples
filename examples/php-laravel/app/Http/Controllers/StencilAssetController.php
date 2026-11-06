<?php

namespace App\Http\Controllers;

use Symfony\Component\HttpFoundation\BinaryFileResponse;

/** Serves the Stencil client bundle from STENCIL_CLIENT_DIR under /stencil/. */
class StencilAssetController
{
    private const TYPES = [
        'js' => 'text/javascript; charset=utf-8',
        'css' => 'text/css; charset=utf-8',
        'map' => 'application/json',
    ];

    public function __invoke(string $path): BinaryFileResponse
    {
        $root = realpath(config('ssr.client_dir'));
        $file = $root ? realpath($root.DIRECTORY_SEPARATOR.$path) : false;

        abort_unless($file && str_starts_with($file, $root.DIRECTORY_SEPARATOR) && is_file($file), 404);

        $type = self::TYPES[pathinfo($file, PATHINFO_EXTENSION)] ?? 'application/octet-stream';

        return response()->file($file, ['Content-Type' => $type]);
    }
}
