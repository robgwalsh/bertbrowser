using BertBrowser.Bench.Cli;

// One executable, several verbs: `run` (Tier A, BenchmarkDotNet), `ui` (Tier B, the harness),
// `startup` (Tier C, the real exe), and the bookkeeping around them — `compare`, `baseline`,
// `report`, `corpus`. Every tier writes the one schema in Core/Benchmarking, and this is the only
// program that writes it.
return BenchCli.Run(args);
