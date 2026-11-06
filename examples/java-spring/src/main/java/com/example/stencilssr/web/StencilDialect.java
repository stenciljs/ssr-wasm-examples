package com.example.stencilssr.web;

import java.io.StringWriter;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;

import org.thymeleaf.TemplateSpec;
import org.thymeleaf.context.Context;
import org.thymeleaf.context.IContext;
import org.thymeleaf.context.ITemplateContext;
import org.thymeleaf.context.IWebContext;
import org.thymeleaf.context.WebContext;
import org.thymeleaf.dialect.AbstractProcessorDialect;
import org.thymeleaf.engine.AttributeName;
import org.thymeleaf.exceptions.TemplateProcessingException;
import org.thymeleaf.model.IProcessableElementTag;
import org.thymeleaf.processor.IProcessor;
import org.thymeleaf.processor.element.AbstractAttributeTagProcessor;
import org.thymeleaf.processor.element.IElementTagStructureHandler;
import org.thymeleaf.templatemode.TemplateMode;

/**
 * Helper mode: a Thymeleaf dialect with one attribute, {@code ssr:render}.
 *
 * <pre>{@code
 * <th:block ssr:render="components :: demo-card"/>
 * }</pre>
 *
 * <p>The value names a Thymeleaf fragment ({@code template :: selector}). The processor
 * renders that fragment with Thymeleaf first, with the page's model, so all the escaping
 * (such as the task JSON in an attribute) is done by the template engine; passes the result to
 * the plugin as a fragment ({@code fullDocument: false}) with the request's handle; and
 * replaces the element with the rendered HTML, unescaped and not processed again.
 */
public class StencilDialect extends AbstractProcessorDialect {

    public static final String PREFIX = "ssr";

    private final SsrRequest ssr;

    /** @param ssr the request-scoped handle holder (a scoped proxy, so it's safe in a singleton) */
    public StencilDialect(SsrRequest ssr) {
        super("Stencil SSR", PREFIX, 1000);
        this.ssr = ssr;
    }

    @Override
    public Set<IProcessor> getProcessors(String dialectPrefix) {
        return Set.of(new RenderProcessor(dialectPrefix, ssr));
    }

    static final class RenderProcessor extends AbstractAttributeTagProcessor {

        private final SsrRequest ssr;

        RenderProcessor(String dialectPrefix, SsrRequest ssr) {
            super(TemplateMode.HTML, dialectPrefix, null, false, "render", true, 1000, true);
            this.ssr = ssr;
        }

        @Override
        protected void doProcess(ITemplateContext context, IProcessableElementTag tag, AttributeName attributeName,
                String value, IElementTagStructureHandler structureHandler) {
            String markup = renderFragment(context, value);
            structureHandler.replaceWith(ssr.renderFragment(markup), false);
        }

        /** Renders {@code template :: selector} with the current page's variables. */
        static String renderFragment(ITemplateContext context, String spec) {
            int sep = spec.indexOf("::");
            if (sep < 0) {
                throw new TemplateProcessingException("ssr:render expects \"template :: selector\", got \"" + spec + "\"");
            }
            String template = spec.substring(0, sep).trim();
            String selector = spec.substring(sep + 2).trim();

            Map<String, Object> vars = new HashMap<>();
            for (String name : context.getVariableNames()) {
                vars.put(name, context.getVariable(name));
            }
            IContext fragmentContext = context instanceof IWebContext web
                    ? new WebContext(web.getExchange(), context.getLocale(), vars)
                    : new Context(context.getLocale(), vars);

            StringWriter out = new StringWriter();
            context.getConfiguration().getTemplateManager().parseAndProcess(
                    new TemplateSpec(template, Set.of(selector), TemplateMode.HTML, null), fragmentContext, out);
            return out.toString().strip();
        }
    }
}
