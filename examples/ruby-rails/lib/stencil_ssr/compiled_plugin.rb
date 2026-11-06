require "extism"

module StencilSsr
  # Compile the plugin once, instantiate it many times.
  #
  # The Extism Ruby SDK (1.0.x) only has Extism::Plugin.new, which compiles the whole module
  # (~6.5 MB of QuickJS + components) on every call. libextism itself has a compile-once API, so
  # this binds those two functions and wraps each new instance in a regular Extism::Plugin, so
  # calls, errors and freeing all go through the SDK as usual.
  class CompiledPlugin
    module Lib
      extend FFI::Library
      ffi_lib "extism"
      attach_function :extism_compiled_plugin_new, %i[pointer uint64 pointer uint64 bool pointer], :pointer
      attach_function :extism_compiled_plugin_free, [:pointer], :void
      attach_function :extism_plugin_new_from_compiled, %i[pointer pointer], :pointer
    end

    def initialize(wasm, wasi: true)
      code = FFI::MemoryPointer.new(:char, wasm.bytesize).put_bytes(0, wasm)
      errmsg = FFI::MemoryPointer.new(:pointer)
      @ptr = Lib.extism_compiled_plugin_new(code, wasm.bytesize, nil, 0, wasi, errmsg)
      raise Extism::Error, take_error(errmsg) if @ptr.null?

      ObjectSpace.define_finalizer(self, self.class.releaser(@ptr))
    end

    # A new, independent plugin instance (an Extism::Plugin).
    def instantiate
      errmsg = FFI::MemoryPointer.new(:pointer)
      ptr = Lib.extism_plugin_new_from_compiled(@ptr, errmsg)
      raise Extism::Error, take_error(errmsg) if ptr.null?

      # Same bookkeeping Extism::Plugin#initialize does, so #call / #free / GC work unchanged.
      Extism::Plugin.allocate.tap do |plugin|
        plugin.instance_variable_set(:@plugin, ptr)
        $PLUGINS[plugin.object_id] = { plugin: ptr }
        ObjectSpace.define_finalizer(plugin, $FREE_PLUGIN)
      end
    end

    def self.releaser(ptr)
      proc { Lib.extism_compiled_plugin_free(ptr) }
    end

    private

    def take_error(errmsg)
      message = errmsg.read_pointer.read_string
      Extism::LibExtism.extism_plugin_new_error_free(errmsg.read_pointer)
      message
    end
  end
end
