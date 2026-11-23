--TEST--
write_report() writes every attributed test and every measured file: a line hit by two tests lists both, unhit lines have empty lists, paths are relative to the root
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app:{PWD}/fixtures/outside
--FILE--
<?php
require __DIR__ . '/fixtures/report.inc';
$app = __DIR__ . '/fixtures/app';
require "$app/calc.php";
require "$app/idle.php";
opcov_run_tests();
\opcov\start();
add(5, 5);
\opcov\stop('Tests\CalcTest::test_add');
\opcov\reset();
\opcov\start();
add(0, 0);
\opcov\stop();
$dir = opcov_report_dir();
\opcov\write_report("$dir/report.json", ['root' => $app]);
$json = file_get_contents("$dir/report.json");
var_dump(str_ends_with($json, "}\n"), substr_count($json, "\n"));
$report = json_decode($json, true);
var_dump($report['files']['calc.php']['sha256'] === hash_file('sha256', "$app/calc.php"));
var_dump($report['generator']['opcov'] === \opcov\version(), $report['generator']['php'] === PHP_VERSION, $report['generator']['zts'] === PHP_ZTS);
echo opcov_masked($json, [__DIR__ => '{DIR}', $report['files']['calc.php']['sha256'] => '{CALC}', $report['files']['idle.php']['sha256'] => '{IDLE}']), "\n";
opcov_remove_dir($dir);
--EXPECT--
bool(true)
int(1)
bool(true)
bool(true)
bool(true)
bool(true)
{
    "schema": "opcov-report/1",
    "generator": {
        "opcov": "string",
        "php": "string",
        "zts": "boolean",
        "vm": "string"
    },
    "settings": {
        "collect_paths": [
            "{DIR}/fixtures/app",
            "{DIR}/fixtures/outside"
        ],
        "exclude_paths": [],
        "exclude_pattern": null
    },
    "paths": {
        "mode": "relative",
        "root": "{DIR}/fixtures/app"
    },
    "tests": [
        "Tests\\CalcTest::test_add",
        "Tests\\CalcTest::test_sign"
    ],
    "files": {
        "calc.php": {
            "sha256": "{CALC}",
            "lines": {
                "4": [
                    0
                ],
                "9": [
                    0,
                    1
                ],
                "10": [
                    1
                ],
                "12": [
                    0
                ],
                "15": [],
                "16": []
            },
            "functions": {
                "{main}@calc.php": {
                    "span": null,
                    "outcomes": [],
                    "loops": []
                },
                "add": {
                    "span": null,
                    "outcomes": [],
                    "loops": []
                },
                "sign": {
                    "span": null,
                    "outcomes": [],
                    "loops": []
                },
                "{closure}@calc.php:15-15": {
                    "span": null,
                    "outcomes": [],
                    "loops": []
                }
            }
        },
        "idle.php": {
            "sha256": "{IDLE}",
            "lines": {
                "4": [],
                "5": [],
                "6": []
            },
            "functions": {
                "{main}@idle.php": {
                    "span": null,
                    "outcomes": [],
                    "loops": []
                },
                "idle": {
                    "span": null,
                    "outcomes": [],
                    "loops": []
                }
            }
        }
    }
}
