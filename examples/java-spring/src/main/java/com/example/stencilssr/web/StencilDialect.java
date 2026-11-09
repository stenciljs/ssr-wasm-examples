package com.example.stencilssr.web;

import java.io.StringWriter;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;

import org.springframework.stereotype.Component;
import org.thymeleaf.TemplateSpec;
import org.thymeleaf.context.ITemplateContext;
import org.thymeleaf.context.IWebContext;
import org.thymeleaf.context.WebContext;
import org.thymeleaf.dialect.AbstractProcessorDialect;
import org.thymeleaf.engine.AttributeName;
import org.thymeleaf.model.IProcessableElementTag;
import org.thymeleaf.processor.IProcessor;
import org.thymeleaf.processor.element.AbstractAttributeTagProcessor;
import org.thymeleaf.processor.element.IElementTagStructureHandler;
import org.thymeleaf.templatemode.TemplateMode;
import org.thymeleaf.web.IWebExchange;

import com.example.stencilssr.ssr.SsrRenderer;

/**
 * Helper mode: a Thymeleaf attribute, {@code ssr:render}, naming a fragment to server-render.
 *
 * <pre>{@code
 * <th:block ssr:render="components :: demo-card"/>
 * }</pre>
 *
 * <p>It renders the fragment with Thymeleaf and the page's model (so the template engine does
 * the escaping, such as the task JSON in an attribute), passes the result to the plugin with
 * {@code {"fullDocument": false}}, and replaces the element with the rendered HTML.
 * Spring Boot adds every dialect bean to its Thymeleaf engine.
 */
@Component
public class StencilDialect extends AbstractProcessorDialect {

    /** Request attribute holding the request's plugin handle. {@link SsrFilter} closes it and skips the page. */
    static final String HANDLE_ATTRIBUTE = SsrRenderer.Handle.class.getName();

    private final SsrRenderer renderer;

    StencilDialect(SsrRenderer renderer) {
        super("Stencil SSR", "ssr", 1000);
        this.renderer = renderer;
    }

    @Override
    public Set<IProcessor> getProcessors(String prefix) {
        return Set.of(new AbstractAttributeTagProcessor(TemplateMode.HTML, prefix, null, false, "render", true, 1000, true) {
            @Override
            protected void doProcess(ITemplateContext context, IProcessableElementTag tag, AttributeName attributeName,
                    String value, IElementTagStructureHandler structureHandler) {
                structureHandler.replaceWith(render(context, value), false); // false: don't process it again
            }
        });
    }

    private String render(ITemplateContext context, String fragment) {
        IWebExchange exchange = ((IWebContext) context).getExchange();

        // Render "template :: selector" with the page's variables.
        String[] spec = fragment.split("::");
        Map<String, Object> vars = new HashMap<>();
        for (String name : context.getVariableNames()) {
            vars.put(name, context.getVariable(name));
        }
        StringWriter markup = new StringWriter();
        context.getConfiguration().getTemplateManager().parseAndProcess(
                new TemplateSpec(spec[0].trim(), Set.of(spec[1].trim()), TemplateMode.HTML, null),
                new WebContext(exchange, context.getLocale(), vars), markup);

        // The first component on the page acquires the request's handle; later ones reuse it.
        var handle = (SsrRenderer.Handle) exchange.getAttributeValue(HANDLE_ATTRIBUTE);
        if (handle == null) {
            handle = renderer.acquire();
            exchange.setAttributeValue(HANDLE_ATTRIBUTE, handle);
        }
        return handle.renderFragment(markup.toString().strip());
    }
}
