module StencilHelper
  # Server-renders the component markup in the block as a fragment and inlines the result:
  #
  #   <%= stencil do %>
  #     <demo-counter start="5" step="2"></demo-counter>
  #   <% end %>
  def stencil(&block)
    StencilSsr.render_fragment(request.env, capture(&block).to_str).html_safe
  end
end
