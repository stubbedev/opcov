use mago_allocator::LocalArena;
use mago_analyzer::Analyzer;
use mago_analyzer::analysis_result::AnalysisResult;
use mago_analyzer::artifacts::AnalysisArtifacts;
use mago_analyzer::error::AnalysisError;
use mago_analyzer::plugin::PluginRegistry;
use mago_analyzer::settings::Settings;
use mago_codex::metadata::CodebaseMetadata;
use mago_codex::reference::SymbolReferences;
use mago_database::file::File;
use mago_names::ResolvedNames;
use mago_names::resolver::NameResolver;
use mago_syntax::cst::Program;
use mago_syntax::parser::parse_file;

pub(crate) struct Context<'a> {
    pub(crate) metadata: &'a CodebaseMetadata,
    pub(crate) registry: &'a PluginRegistry,
    pub(crate) settings: &'a Settings,
}

pub(crate) fn with_analysis<R>(
    context: &Context<'_>,
    file: &File,
    inspect: impl FnOnce(
        &Program<'_>,
        &ResolvedNames<'_>,
        Result<&AnalysisArtifacts, AnalysisError>,
    ) -> R,
) -> R {
    let arena = LocalArena::new();
    let program = parse_file(&arena, file);
    let names = NameResolver::new(&arena).resolve(program);
    let mut result = AnalysisResult::new(SymbolReferences::new());
    let analyzer = Analyzer::new(
        &arena,
        file,
        &names,
        context.metadata,
        context.registry,
        context.settings.clone(),
    );
    let analysed = analyzer.analyze_with_artifacts(program, &mut result);
    return inspect(program, &names, analysed.as_ref().map_err(Clone::clone));
}
