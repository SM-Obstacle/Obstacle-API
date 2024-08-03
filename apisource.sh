#!/bin/bash

__wd=$(pwd)
__dir=$(dirname $0)

__api_pid=
__website_pid=
__obs_started=

# Returns 0 if the PID $1 is not running, 1 otherwise
__is_pid_stopped() {
    if ps -p $1 >/dev/null 2>&1; then return 1; fi
    return 0
}

# Waits for the PID $1 makes a fork, then returns its PID
__wait_for_fork() {
    # Avoids waiting forever
    if __is_pid_stopped $1; then return; fi
    local cpid=$(pgrep -P $1)
    until [[ -n $cpid ]]; do
        if __is_pid_stopped $1; then return; fi
        cpid=$(pgrep -P $1)
    done
    echo $cpid
}

startobs() {
    local firstwd=$(pwd)
    local dir="$__wd/$__dir"
    cd $dir

    if [[ -n $__api_pid || -n $__website_pid || -n $__obs_started ]]; then
        echo "Error: Obstacle services already running."
        statusobs
        cd $firstwd
        return 1
    fi

    if [[ $1 == "gql" ]]; then
        local output_gql_arg="-F gql_schema"
    fi
    cargo run --bin game-api $output_gql_arg > ./etc/api_out 2>&1 &
    __api_pid=$!
    echo "Obstacle API PID: $__api_pid"

    if [[ $1 == "gql" ]]; then
        until [[ -f ./etc/api_last_gql_schema ]]; do
            sleep 1
        done

        local last_schema=$(cat ./etc/api_last_gql_schema)
        local schema_file="schemas/$last_schema"
        cp $schema_file website/schema.graphql
        rm $schema_file
        rm ./etc/api_last_gql_schema
        cd website
        yarn compile
        cd ..
    fi

    cd website
    yarn dev > ../etc/web_out 2>&1 &
    local pid=$!
    local fork=$(__wait_for_fork $pid | tail -n 1)
    __website_pid=$(__wait_for_fork $fork | tail -n 1)
    echo "Obstacle website PID: $__website_pid"

    cd $firstwd
    __obs_started=_
}

attachobs() {
    local firstwd=$(pwd)
    local dir="$__wd/$__dir"
    cd $dir

    local code=0

    local arg=$1
    if [[ -z $arg ]]; then arg=api; fi

    case $arg in
    api)
        if [[ -z $__obs_started || -z $__api_pid || ! -f ./etc/api_out ]]; then
            echo "Error: Obstacle API is not running."
            code=1
        else
            tail -f ./etc/api_out
        fi
        ;;
    website)
        if [[ -z $__obs_started || -z $__website_pid || ! -f ./etc/web_out ]]; then
            echo "Error: Obstacle website is not running."
            code=1
        else
            tail -f ./etc/web_out
        fi
        ;;
    *)
        echo "Error: either attach to \`api\` (default) or \`website\`."
        code=1
        ;;
    esac

    cd $firstwd
    return $code
}

statusobs() {
    if [[ -z $__obs_started ]]; then
        echo "Obstacle services not started."
        return 1
    fi

    if [[ -n $__api_pid ]]; then
        echo "Obstacle API is running on PID: $__api_pid."
    else
        echo "Obstacle API is not running."
    fi

    if [[ -n $__website_pid ]]; then
        echo "Obstacle website is running on PID: $__website_pid."
    else
        echo "Obstacle website is not running."
    fi
}

stopobs() {
    local firstwd=$(pwd)
    local dir="$__wd/$__dir"
    cd $dir

    if [[ -z $__obs_started ]]; then
        echo "Error: you need to run startobs before."
        __api_pid=
        __website_pid=
        cd $firstwd
        return 1
    fi

    __obs_started=

    if [[ -n $__api_pid && $(! kill $__api_pid >/dev/null 2>&1) ]]; then
        echo "Warning: the Obstacle API (PID: $__api_pid) is not running" >&2
    fi

    if [[ -n $__website_pid && $(! kill $__website_pid >/dev/null 2>&1) ]]; then
        echo "Warning: the Obstacle website (PID: $__website_pid) is not running" >&2
    fi

    rm ./etc/*_out >/dev/null 2>&1

    __api_pid=
    __website_pid=

    cd $firstwd
}

restartobs() {
    stopobs && startobs $1
}