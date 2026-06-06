global _


def get_command_line_parameters():
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "-m",
        "--start-minimized",
        help=_("Start application minimized"),
        action="store_true",
    )
    parser.add_argument(
        "-s",
        "--snippets-on-invoke",
        help=_("When app is invoked by shortcut, start with snippets mode instead of commands"),
        action="store_true",
    )

    parser.add_argument(
        "-q",
        "--quiet",
        help=_("Disable all sounds"),
        action="store_true",
    )

    parser.add_argument(
        "-c",
        "--commands",
        help=_("specify commands file to use, default is commands.json"),
        action="store",
        default="commands.json",
    )

    parser.add_argument(
        "-l",
        "--steam-library",
        help=_("specify custom Steam library path"),
        action="store",
        default=None,
    )

    args = parser.parse_args()

    return args
