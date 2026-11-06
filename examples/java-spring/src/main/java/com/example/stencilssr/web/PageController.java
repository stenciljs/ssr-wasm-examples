package com.example.stencilssr.web;

import java.util.List;

import org.springframework.http.MediaType;
import org.springframework.stereotype.Controller;
import org.springframework.ui.Model;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.ResponseBody;

import com.fasterxml.jackson.annotation.JsonInclude;

import tools.jackson.databind.json.JsonMapper;

/**
 * The pages. Neither handler knows about SSR: {@code /} is rendered by {@link SsrFilter},
 * {@code /helper} by the {@code ssr:render} attributes in its template.
 */
@Controller
public class PageController {

    static final String FRAMEWORK = "Spring Boot";

    @JsonInclude(JsonInclude.Include.NON_NULL) // the third task has no "done" key at all
    record Task(String title, Boolean done) {}

    static final List<Task> TASKS = List.of(
            new Task("Write components", true),
            new Task("Render with <wasm> & \"friends\"", false),
            new Task("Hydrate on the client", null));

    private final String tasksJson;

    PageController(JsonMapper json) {
        // Plain JSON: Jackson doesn't escape <, > or & by default, and HTML-escaping it is the
        // template's job (th:attr escapes it into the attribute).
        this.tasksJson = json.writeValueAsString(TASKS);
    }

    @GetMapping("/")
    String index(Model model) {
        page(model);
        return "index";
    }

    @GetMapping("/helper")
    String helper(Model model) {
        page(model);
        return "helper";
    }

    @GetMapping(value = "/health", produces = MediaType.TEXT_PLAIN_VALUE)
    @ResponseBody
    String health() {
        return "ok";
    }

    private void page(Model model) {
        model.addAttribute("framework", FRAMEWORK);
        model.addAttribute("tasksJson", tasksJson);
    }
}
