use axum::{Router, response::Html, routing::get};
use leptos::prelude::*;

/// Draws a quiet architectural sketch without downloading an image or icon font.
#[component]
fn Foundation() -> impl IntoView {
    view! {
        <svg viewBox="0 0 360 360" fill="none" aria-hidden="true" class="w-full max-w-sm">
            <defs>
                <pattern id="grid" width="24" height="24" patternUnits="userSpaceOnUse">
                    <path d="M 24 0 L 0 0 0 24" stroke="currentColor" stroke-width="0.5"/>
                </pattern>
            </defs>
            <rect x="12" y="12" width="336" height="336" fill="url(#grid)" class="text-line"/>
            <path d="M36 276H324M72 312V48M288 312V48" stroke="currentColor" stroke-dasharray="3 6" class="text-muted/40"/>
            <path d="M78 246L180 305L282 246L180 187L78 246Z" class="fill-ink/5"/>
            <path d="M84 224L180 280V170L84 114V224Z" class="fill-paper stroke-ink" stroke-width="1.5"/>
            <path d="M180 280L276 224V114L180 170V280Z" class="fill-line/60 stroke-ink" stroke-width="1.5"/>
            <path d="M84 114L180 58L276 114L180 170L84 114Z" class="fill-paper stroke-ink" stroke-width="1.5"/>
            <path d="M132 142V252M228 142V252M84 169L180 225L276 169" class="stroke-ink/30"/>
            <path d="M132 86L228 142M228 86L132 142" class="stroke-ink/30"/>
            <path d="M180 58V170M84 114L180 170L276 114" class="stroke-accent" stroke-width="2"/>
            <circle cx="180" cy="58" r="4" class="fill-accent"/>
            <circle cx="180" cy="170" r="4" class="fill-accent"/>
            <path d="M36 48V36H48M312 36H324V48M324 312V324H312M48 324H36V312" class="stroke-muted/60"/>
        </svg>
    }
}

/// Renders a complete, responsive document using only embedded assets and system fonts.
#[component]
fn Page() -> impl IntoView {
    view! {
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta name="description" content="A new home is taking shape. Check back soon."/>
                <meta name="theme-color" content="#f6f4ee"/>
                <title>"Under construction"</title>
                <style inner_html=include_str!("site.css")></style>
            </head>
            <body class="min-h-svh bg-paper font-sans text-ink antialiased selection:bg-accent/20">
                <div class="mx-auto flex min-h-svh max-w-6xl flex-col px-6 sm:px-10 lg:px-14">
                    <header class="flex flex-wrap items-center justify-between gap-4 border-b border-line py-7 sm:py-9">
                        <span class="flex items-center gap-3 text-sm font-medium tracking-tight">
                            <span aria-hidden="true" class="grid size-8 grid-cols-2 gap-1 p-1">
                                <span class="rounded-[2px] bg-ink"></span>
                                <span class="rounded-[2px] bg-accent"></span>
                                <span class="rounded-[2px] bg-ink/25"></span>
                                <span class="rounded-[2px] bg-ink"></span>
                            </span>
                            "A new beginning."
                        </span>
                        <span class="flex items-center gap-2 font-mono text-[10px] tracking-widest text-muted uppercase sm:text-xs">
                            <span aria-hidden="true" class="size-1.5 rounded-full bg-accent"></span>
                            "Work in progress"
                        </span>
                    </header>
                    <main class="grid flex-1 items-center gap-10 py-14 sm:py-20 lg:grid-cols-[1.3fr_1fr] lg:gap-16 lg:py-24">
                        <div>
                            <p class="mb-7 font-mono text-xs tracking-[0.2em] text-accent uppercase">"Room for something new"</p>
                            <h1 class="max-w-xl text-[clamp(3.25rem,8.5vw,7rem)] leading-[0.98] font-semibold tracking-[-0.065em]">
                                "Under"<br/>"construction."
                            </h1>
                            <div aria-hidden="true" class="my-8 h-px w-12 bg-accent"></div>
                            <p class="max-w-sm text-base leading-7 text-muted sm:text-lg sm:leading-8">
                                "A new home is taking shape. A few things are still being put in place. Check back soon."
                            </p>
                        </div>
                        <figure class="mx-auto flex w-full max-w-sm flex-col items-center lg:pt-6">
                            <Foundation/>
                            <figcaption class="mt-4 font-mono text-[10px] tracking-[0.16em] text-muted uppercase">
                                "One piece at a time."
                            </figcaption>
                        </figure>
                    </main>
                    <footer class="flex flex-wrap items-center justify-between gap-3 border-t border-line py-6 text-xs text-muted sm:py-8">
                        <p>"Thanks for stopping by."</p>
                        <p class="font-mono text-[10px] tracking-widest uppercase">"See you soon."</p>
                    </footer>
                </div>
            </body>
        </html>
    }
}

/// Produces server-rendered HTML with the compiled stylesheet embedded in the document.
async fn index() -> Html<String> {
    Html(format!("<!DOCTYPE html>{}", view! { <Page/> }.to_html()))
}

/// Confirms the HTTP server is ready to receive requests.
async fn health() -> &'static str {
    "ok\n"
}

/// Builds the public page and readiness routes for a standalone HTTP process.
pub fn router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(health))
}
