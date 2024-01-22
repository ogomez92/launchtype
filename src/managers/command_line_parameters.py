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
        "-c",
        "--commands",
        help=_("specify commands file to use, default is commands.json"),
        action="store",
        default="commands.json",
    )

    args = parser.parse_args()

    return args
