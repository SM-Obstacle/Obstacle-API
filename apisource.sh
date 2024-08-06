#!/bin/bash

__wd=$(pwd)
__dir=$(dirname $0)

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

update_gql() {
    local firstwd=$(pwd)
    local dir="$__wd/$__dir"
    cd $dir

    cargo run --bin game-api -F gql_schema > ./etc/api_out 2>&1 &

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

    cd $firstwd
}

startobs() {
    local firstwd=$(pwd)
    local dir="$__wd/$__dir"
    cd $dir

    if [[ $1 == "gql" ]]; then
        local output_gql_arg="-F gql_schema"
    fi
    cargo run --bin game-api $output_gql_arg > ./etc/api_out 2>&1 &

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

    cd $firstwd
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
        if [[ ! -f ./etc/api_out ]]; then
            echo "Error: Obstacle API is not running."
            code=1
        else
            tail -f ./etc/api_out
        fi
        ;;
    website)
        if [[ ! -f ./etc/web_out ]]; then
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